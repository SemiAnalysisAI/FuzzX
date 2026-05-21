# w300: foldICmpMulConstant sign-bit-test fold missing `MulC == 0` guard

## Location
`amdgpu/third_party/llvm-project/llvm/lib/Transforms/InstCombine/InstCombineCompares.cpp:2206-2210`

## Issue (latent / robustness)
The fold:
```cpp
// (X * +MulC) < 0 --> X < 0
// (X * -MulC) < 0 --> X > 0
if (isSignTest(Pred, C) && Mul->hasNoSignedWrap()) {
  if (MulC->isNegative())
    Pred = ICmpInst::getSwappedPredicate(Pred);
  return new ICmpInst(Pred, X, ConstantInt::getNullValue(MulTy));
}
```
fires before the `MulC->isZero()` guard at line 2212. If `MulC == 0`,
`MulC->isNegative()` is false, so `Pred` is left as-is (e.g., SLT). The fold
returns `icmp slt X, 0`. But the original `mul nsw X, 0` is `0`, and
`0 slt 0` is `false`. For any negative X, the rewrite gives `true` —
a miscompile.

In current LLVM, `simplifyMul` folds `X * 0 -> 0` before this code is reached
(via the simplification step at the top of `foldICmpInstWithConstant`), so
this path is not observably triggered by handwritten IR (we verified: scalar,
splat vector, even with `noundef`, all constant-fold to `false` before
`foldICmpMulConstant` runs). The bug is therefore a latent robustness issue
rather than an exploitable miscompile today.

## Fix
Move the `if (MulC->isZero()) return nullptr;` check (currently at line 2212)
above the sign-bit-test fold at 2206, or add a `&& !MulC->isZero()` to the
condition at 2206.

## Test (currently constant-folds upstream; included only to document the
hypothetical pre-condition)
```ll
define i1 @f(i32 %x) {
  %m = mul nsw i32 %x, 0
  %c = icmp slt i32 %m, 0
  ret i1 %c
}
```
Expected after instcombine: `ret i1 false`.
Observed: `ret i1 false` (because `simplifyMul` fires first; the bug is
guarded against in practice but not at the site itself).

## Notes
- Severity: latent. Not a miscompile under default x86 -O2 today.
- If `simplifyMul` is ever reordered or a path is added that bypasses it,
  this becomes a miscompile.
- Same `MulC == 0` guard is correctly placed at line 2212 for the rest of
  this function — the sign-test fold above is the only one out of order.
