# JumpThreading tryToUnfoldSelect(SwitchInst, BB) unfolds without verifying a fold is enabled

## Location
`llvm/lib/Transforms/Scalar/JumpThreading.cpp:2836-2860` in
`JumpThreadingPass::tryToUnfoldSelect(SwitchInst *, BasicBlock *)`.

```cpp
bool JumpThreadingPass::tryToUnfoldSelect(SwitchInst *SI, BasicBlock *BB) {
  PHINode *CondPHI = dyn_cast<PHINode>(SI->getCondition());
  if (!CondPHI || CondPHI->getParent() != BB)
    return false;

  for (unsigned I = 0, E = CondPHI->getNumIncomingValues(); I != E; ++I) {
    BasicBlock *Pred = CondPHI->getIncomingBlock(I);
    SelectInst *PredSI = dyn_cast<SelectInst>(CondPHI->getIncomingValue(I));
    if (!PredSI || PredSI->getParent() != Pred || !PredSI->hasOneUse())
      continue;
    UncondBrInst *PredTerm = dyn_cast<UncondBrInst>(Pred->getTerminator());
    if (!PredTerm)
      continue;

    unfoldSelectInstr(Pred, BB, PredSI, CondPHI, I);
    return true;
  }
  return false;
}
```

## Bug
Compare with the `CmpInst` overload at line 2874:

```cpp
Constant *LHSRes = LVI->getPredicateOnEdge(... SI->getOperand(1) ...);
Constant *RHSRes = LVI->getPredicateOnEdge(... SI->getOperand(2) ...);
if ((LHSRes || RHSRes) && LHSRes != RHSRes) {
  unfoldSelectInstr(Pred, BB, SI, CondLHS, I);
  return true;
}
```

The `CmpInst` overload only unfolds when one (and only one) arm of the
select would let LVI fold the consuming branch. The `SwitchInst` overload
does NOT do this LVI check — it unfolds any one-use select feeding the
switch condition. The comment on line 2846-2848 acknowledges this is a
simplification.

## Severity
**Compile-time / code-size**: missed cleanup. Unfolding always introduces
an additional basic block (NewBB) and converts the predecessor terminator
from an unconditional to a conditional branch. If neither select arm
enables the switch to fold downstream, this is pure pessimisation:
+1 branch, +1 block, no follow-up simplification.

This is a missed-opt soundness issue rather than a wrong-code bug — the
transformation itself is semantically equivalent (select-on-cond ->
branch-on-cond, both well-defined as long as the select cond is). But
unlike most JumpThreading transforms, no profitability check guards it.

## Reproducer
`/tmp/w85/sw1.ll`:
```llvm
target triple = "x86_64-unknown-linux-gnu"
declare i32 @opaque()
define i32 @f(i1 %x, i32 %a, i32 %b) {
entry:
  br i1 %x, label %pred, label %other
pred:
  %nondetc = call i32 @opaque()
  %c = icmp slt i32 %nondetc, 0
  %s = select i1 %c, i32 %a, i32 %b
  br label %bb
other:
  %o = call i32 @opaque()
  br label %bb
bb:
  %p = phi i32 [ %s, %pred ], [ %o, %other ]
  switch i32 %p, label %def [ i32 0, label %z
                              i32 1, label %one
                              i32 2, label %two ]
z: ret i32 100
one: ret i32 200
two: ret i32 300
def: ret i32 400
}
```

After `opt -passes=jump-threading -S`, the select is unfolded into a new
block `select.unfold` with a conditional branch on `%c` — but the switch
gains no folding opportunity because LVI cannot resolve `%a` or `%b`
against any switch case.

## Status
Source-confirmed. Missed-optimisation / code-size pessimisation rather
than a wrong-code bug. Worth a small upstream patch mirroring the
`CmpInst` overload's `LVI` check: only unfold if at least one arm makes
the switch case foldable.
