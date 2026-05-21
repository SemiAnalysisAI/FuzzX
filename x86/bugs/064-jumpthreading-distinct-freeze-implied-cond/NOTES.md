# JumpThreading processImpliedCondition: treats two distinct freezes of same operand as equal-valued

## Location
`llvm/lib/Transforms/Scalar/JumpThreading.cpp:1180-1184` (inside
`JumpThreadingPass::processImpliedCondition`).

```cpp
// If the branch condition of BB (which is Cond) and CurrentPred are
// exactly the same freeze instruction, Cond can be folded into CondIsTrue.
if (!Implication && FICond && isa<FreezeInst>(PBI->getCondition())) {
  if (cast<FreezeInst>(PBI->getCondition())->getOperand(0) ==
      FICond->getOperand(0))
    Implication = CondIsTrue;
}
```

## Bug
The comment says "exactly the same freeze instruction" but the check only
compares **operands** of the two freeze instructions. Because we entered this
branch with `FICond->hasOneUse()` (line 1156), the predecessor's freeze
(`PBI->getCondition()`) is by definition a **different** `FreezeInst` from
`FICond`.

Per the LLVM LangRef, every `freeze` of a poison/undef value picks an
arbitrary value *independently*. Two distinct `freeze i1 %poison_cmp`
instructions are not guaranteed to agree. JumpThreading concludes they
must, and prunes a successor that is in fact reachable.

This matches the bug-pattern category "JumpThreading folds a branch using
LVI/SCEV info that becomes invalid after a code motion" — here, the
underlying invariant (sameness of two freeze observations) was never valid
to begin with.

## Reproducer
`/tmp/w33_jt_freeze2.ll`:
```llvm
define i32 @f(i32 %a) {
entry:
  %s = add nsw i32 %a, 1            ; poison when %a == INT_MAX
  %cmp = icmp slt i32 %s, 0         ; poison-propagating
  br label %pred
pred:
  %fz1 = freeze i1 %cmp
  br i1 %fz1, label %mid, label %exit
mid:
  %fz2 = freeze i1 %cmp
  br i1 %fz2, label %T, label %F
T: ret i32 1
F: ret i32 2
exit: ret i32 0
}
```

`opt -passes=jump-threading -S` produces:
```llvm
pred:
  %s = add nsw i32 %a, 1
  %cmp = icmp slt i32 %s, 0
  %fz1 = freeze i1 %cmp
  br i1 %fz1, label %T, label %exit   ; mid and F are gone; F unreachable
```

The fold of "pred -> mid -> T" was justified by treating the two freezes as
equal. After the fold there is no path returning 2.

## Expected behavior
The fold should only apply if `PBI->getCondition() == FICond` (literally the
same SSA Value), or if `FICond->getOperand(0)` is already proven
`isGuaranteedNotToBeUndefOrPoison` (in which case the freezes are no-ops
and folding is sound).

## Severity
Miscompile of programs that branch on freeze(potential-poison) more than
once. Real-world frequency is low (idiomatic code freezes once and reuses);
but the pattern is reachable from SimplifyCFG / CodeGenPrepare promotion of
freeze. Worth verifying with extra LangRef interpretation before filing
upstream — there's a (small) chance LLVM informally treats freeze of the
same operand as "same value because the operand is one fixed SSA value",
but the LangRef wording on `freeze` says explicitly:
"freeze returns an arbitrary, but fixed, value of type ty."  Each freeze is
an independent choice.

## Status
Source-confirmed + transform-confirmed. Runtime-confirmed pending (cannot
construct an executable repro without controlling the nondeterministic
freeze choice; the bug is a poison-refinement that *permits* the compiler
to pick a wrong answer rather than guaranteeing one).
