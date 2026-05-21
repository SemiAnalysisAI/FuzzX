# SCEVRewriteVisitor default `visitAddExpr`/`visitMulExpr` silently drop NUW/NSW flags

File: `llvm/include/llvm/Analysis/ScalarEvolutionExpressions.h`
Functions: `SCEVRewriteVisitor::visitAddExpr` (lines 897-905) and
`SCEVRewriteVisitor::visitMulExpr` (lines 907-915).

## The transform

`SCEVRewriteVisitor<SC>` is the CRTP base used by virtually every SCEV
rewriter in tree (`SCEVParameterRewriter`, `SCEVLoopAddRecRewriter`,
`SCEVInitRewriter`, `SCEVPostIncRewriter`, `SCEVBackedgeConditionFolder`,
`SCEVShiftRewriter`, `SCEVPredicateRewriter`, `SCEVLoopGuardRewriter`,
`SCEVMapper`, ...). The default Add/Mul handlers are:

```cpp
const SCEV *visitAddExpr(const SCEVAddExpr *Expr) {
  SmallVector<SCEVUse, 2> Operands;
  bool Changed = false;
  for (const SCEV *Op : Expr->operands()) {
    Operands.push_back(((SC *)this)->visit(Op));
    Changed |= Op != Operands.back();
  }
  return !Changed ? Expr : SE.getAddExpr(Operands);
}

const SCEV *visitMulExpr(const SCEVMulExpr *Expr) {
  ...
  return !Changed ? Expr : SE.getMulExpr(Operands);
}
```

Note: `getAddExpr(Operands)` and `getMulExpr(Operands)` default to
`SCEV::FlagAnyWrap` — **the original `Expr->getNoWrapFlags()` is discarded.**

`SE.getAddExpr` / `SE.getMulExpr` call `StrengthenNoWrapFlags`
(`ScalarEvolution.cpp` lines 2540-2620) which can re-derive flags only via
range analysis on the operands; that often falls short of recovering an
NUW/NSW that was previously proved via a more contextual argument (e.g.,
"this came from `add nuw` in IR"). For example,
`StrengthenNoWrapFlags(scAddExpr, ...)` at lines 2567-2598 only adds NUW/NSW
when the leading operand is a constant and the operand[1] range fits.
Anything that needed contextual NUW (originally proved via a `mustprogress`
loop, an `assume`, or a guarding instruction) is irrevocably lost.

## Self-confirming evidence in tree

`SCEVCastSinkingRewriter` (in `lib/Analysis/ScalarEvolution.cpp` lines
1136-1146) explicitly overrides `visitAddExpr`:

```cpp
const SCEV *visitAddExpr(const SCEVAddExpr *Expr) {
  // Preserve wrap flags on rewritten SCEVAddExpr, which the default
  // implementation drops.
  SmallVector<SCEVUse, 2> Operands;
  bool Changed = false;
  for (SCEVUse Op : Expr->operands()) {
    Operands.push_back(visit(Op.getPointer()));
    Changed |= Op.getPointer() != Operands.back();
  }
  return !Changed ? Expr : SE.getAddExpr(Operands, Expr->getNoWrapFlags());
}
```

The comment *"Preserve wrap flags on rewritten SCEVAddExpr, which the default
implementation drops"* admits the default behavior is wrong/lossy. Yet only
one of the eleven rewriters overrides it — every other consumer silently
loses NUW/NSW on rewritten Add/Mul expressions.

## Why this is in-scope for x86

Many downstream lowering decisions hinge on SCEV NUW:
- `IndVarSimplify`'s widening (`-indvars`) uses NUW to decide whether a
  smaller IV's `zext` is safe to hoist into a wider IV without a runtime
  check. After `SCEVPostIncRewriter` or `SCEVLoopGuardRewriter` quietly
  strips NUW from an intermediate Add, widening gives up, leading to extra
  cmp/branch on x86 (perf regression).
- LoopVectorize uses SCEV NUW on stride expressions when deciding whether
  a memcheck is needed. A stripped-flag rewrite can cause an unnecessary
  predicate to be emitted, AND - in the inverse direction - if the rewritten
  expression is hashed/canonicalized into an existing flag-less SCEV that a
  *different* user previously decorated with assumed-NUW, the consumer's
  cached belief is overwritten without it noticing.

The mechanism is purely a SCEV-analysis pollution; the user-visible
miscompile manifests at x86 codegen time as either suboptimal trip count
arithmetic (lost-opt) or as wrong trip count in a vectorized loop when an
adjacent `visitAddRecExpr` ALSO mis-propagates flags (see companion
candidate w487, where the *opposite* direction — keeping flags blindly —
produces wrong-flagged SCEVs that lie to downstream consumers).

## How to repro / hunt

Crafting a fuzz IR that survives all the way to wrong-codegen requires
finding a downstream pass that (a) reads SCEV NUW on the rewritten expr and
(b) takes a sound shortcut based on it. A profitable corpus angle is:

* A loop with `mustprogress` and an inner IV computed as
  `add nuw i32 %inner, %x` where `%x` comes from outside the loop.
* An outer expression that triggers `SCEVPostIncRewriter` (use the IV's
  post-inc value in an out-of-loop GEP or compare).
* Compare SCEV print before vs after `indvars` for the inner add; the
  rewritten variant lacks NUW even though the operand-by-operand
  re-derivation in `StrengthenNoWrapFlags` cannot prove it.

## Status: source-confirmed (in-tree comment acknowledges the bug).

The companion check that confirms a working rewriter (`SCEVCastSinkingRewriter`'s
override) demonstrates the fix shape: the same overrides need to be added to
the base class so every rewriter inherits them, or at minimum to the rewriters
whose semantics preserve the original mathematical identity of the expression
(which is true for most: parameter substitution, post-inc, init, backedge-
condition-folding, predicates, etc.).
