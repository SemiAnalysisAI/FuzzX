# NewGVN performSymbolicEvaluation passes empty FMF to simplifySelectInst

**File:** `llvm/lib/Transforms/Scalar/NewGVN.cpp:1216` (in
`NewGVN::performSymbolicEvaluation`'s select path)

## Reasoning

```
} else if (isa<SelectInst>(I)) {
  if (isa<Constant>(E->getOperand(0)) ||
      E->getOperand(1) == E->getOperand(2)) {
    ...
    Value *V = simplifySelectInst(E->getOperand(0), E->getOperand(1),
                                  E->getOperand(2), FastMathFlags(), Q);
    ...
  }
```

A FP `select` may carry fast-math flags (e.g., `select nnan`). NewGVN here
hands `FastMathFlags()` (empty) to `simplifySelectInst` instead of
`cast<SelectInst>(I)->getFastMathFlags()`. This is a missed optimization
rather than a miscompile: passing fewer FMF only blocks simplifications, it
cannot enable an unsound one. Still worth filing because (a) every other
simplify caller in this file (`simplifyBinOp` line 1120, 1222) similarly
omits FMF for FP binops, and (b) if the call were ever reorganised so the
simplified `V` is committed without re-applying FMF, an unsound result could
sneak in.

Note: `simplifyBinOp(Opcode, A, B, Q)` (no FMF overload) is called for `FAdd
/ FSub / FMul / FDiv / FRem / FNeg`. The FMF-aware overload exists. Using the
FMF-aware one would allow `fadd nnan` of a known-NaN side to simplify,
matching GVN's behavior.

## IR repro (illustrates the missed simplification)

```
define double @f(i1 %c, double %x) {
  %s = select nnan i1 %c, double 0x7FF8000000000000, double %x ; NaN, x
  ret double %s
}
```

Expected: `select nnan` says result is never NaN, so when one arm is a
constant NaN the optimizer should fold to `%x`. NewGVN does not, because
`simplifySelectInst` is called with empty FMF and so cannot exploit `nnan`.

## Not a miscompile

This is a soundness-preserving omission (always pessimizing). Filed as a
worker-31 finding because the file's responsibilities under the bug-hunt
include FP-flag-related CSE/VN issues.
