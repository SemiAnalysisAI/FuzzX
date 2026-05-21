# SCEVRewriteVisitor::visitAddRecExpr blindly preserves NUW/NSW after operand substitution

File: `llvm/include/llvm/Analysis/ScalarEvolutionExpressions.h`
Function: `SCEVRewriteVisitor::visitAddRecExpr`, lines 924-934.

## The transform

The default AddRec handler in the CRTP rewriter base:

```cpp
const SCEV *visitAddRecExpr(const SCEVAddRecExpr *Expr) {
  SmallVector<SCEVUse, 2> Operands;
  bool Changed = false;
  for (const SCEV *Op : Expr->operands()) {
    Operands.push_back(((SC *)this)->visit(Op));
    Changed |= Op != Operands.back();
  }
  return !Changed ? Expr
                  : SE.getAddRecExpr(Operands, Expr->getLoop(),
                                     Expr->getNoWrapFlags());
}
```

`Expr->getNoWrapFlags()` returns the **original** AddRec's NUW/NSW. These
were proved for the *original* `{Start, +, Step}<L>` based on the original
`Start` and `Step` values. When the rewriter substitutes a different
`Start'` and/or `Step'` (the whole point of `SCEVParameterRewriter`,
`SCEVPostIncRewriter`, `SCEVLoopAddRecRewriter`, `SCEVPredicateRewriter`,
`SCEVLoopGuardRewriter`, etc.), there is **no guarantee** the rewritten
`{Start', +, Step'}<L>` still has those wrap properties.

`SE.getAddRecExpr(Operands, L, Flags)` at
`ScalarEvolution.cpp:3795-3870` *trusts* the caller's flags. It calls
`StrengthenNoWrapFlags(this, scAddRecExpr, Operands, Flags)` (line 3822)
which is documented and implemented to **only add** flags it can newly
prove — it never weakens claimed flags. The result is uniqued/cached in
`UniqueSCEVs` with the over-strong flags baked in, contaminating subsequent
lookups for the same `{Start', +, Step'}<L>` triple.

## Concrete unsoundness path

Consumer: `SCEVLoopAddRecRewriter::visitAddRecExpr` (lines 1036-1047), used
to re-target an AddRec to a different loop. The original code:

```cpp
const SCEV *visitAddRecExpr(const SCEVAddRecExpr *Expr) {
  SmallVector<SCEVUse, 2> Operands;
  for (SCEVUse Op : Expr->operands())
    Operands.push_back(visit(Op));

  const Loop *L = Expr->getLoop();
  auto It = Map.find(L);
  if (It == Map.end())
    return SE.getAddRecExpr(Operands, L, Expr->getNoWrapFlags());
  ...
}
```

Suppose the rewriter has substituted an `Operands[1]` (the step) for one
that came from a *different* loop or context. The original `<nsw>` flag was
justified by, e.g., a guard on the original Step's range; the new Step may
have no such guard. The cached AddRec is now marked `<nsw>` despite being
able to wrap. Any downstream user of the rewritten expression (e.g.,
`howManyLessThans`, IV widening, `LoopVectorize`'s stride proofs) will treat
the wrap as impossible and may delete the latch / overflow path.

## Self-confirming evidence

The base class's `visitAddExpr` and `visitMulExpr` (lines 897-915) take the
opposite — also-buggy — direction: they discard flags. The fact that the
authors had to write a specialized `SCEVCastSinkingRewriter::visitAddExpr`
(`ScalarEvolution.cpp:1136-1145`, with the comment *"Preserve wrap flags on
rewritten SCEVAddExpr, which the default implementation drops"*) shows the
default Add/Mul behavior is acknowledged as unwanted. But the AddRec
counterpart is the inverse: too-aggressive flag preservation, with no
audit on whether the rewritten triple still justifies them.

## Why this is in-scope for x86

A poisoned `<nsw>` AddRec is consumed by:
- `IndVarSimplify` widening, which inserts an `add nsw` IR instruction whose
  poison is undefined behavior; LLVM's `simplifyAndDCE` may then propagate
  poison through to a select / branch and produce wrong-answer x86 code.
- `LoopVectorize`, which can elide a runtime overflow check on the strided
  GEP and let an out-of-bounds index reach a vector store.

These are end-user observable as wrong-answer machine code at `-O2`.

## How to hunt

Search the corpus for fuzz IR where:
- A loop's IV's NSW/NUW was proven via a loop-invariant range guard outside
  the loop (e.g., `assume(x < 100)` followed by an IV like
  `{phi, +, x}<nsw>`).
- Another pass triggers SCEV rewriting that substitutes `x` with a value
  whose range guard does NOT apply in the new context (e.g., the rewriter
  is `SCEVParameterRewriter` mapping `x → y` from a wider context).
- After `-passes=indvars`, observe a widened `add nsw` whose operand
  range allows actual NSW wrap.

## Status: source-confirmed (asymmetry with sibling Add/Mul handlers).

Mechanically certain that the AddRec rewrite path keeps flags without
auditing operand substitution. End-to-end miscompile requires fuzzing a
specific rewriter consumer to trigger.
