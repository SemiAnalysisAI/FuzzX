# 206 — SimplifyLibCalls `optimizeFMod` sets `nnan` on the synthesized `frem` based on a NO-ERRNO proof, not a NO-NAN proof

Component: `llvm/lib/Transforms/Utils/SimplifyLibCalls.cpp` lines ~2854-2880

`optimizeFMod` proves "no errno" by showing `x` is not `±Inf` AND `y` is not zero, then names the proof boolean `IsNoNan` and unconditionally sets `nnan` on the replacement `frem`:

```cpp
if (Known0.isKnownNeverInfinity()) {            // x not Inf
  ...
  IsNoNan = Known1.isKnownNeverLogicalZero(...); // y not 0
}
if (IsNoNan) {
  Value *FRem = B.CreateFRemFMF(...);
  FRemI->setHasNoNaNs(true);                     // BUG: not justified
}
```

NaN is a legal input to `fmod` per C99 (errno unchanged). After this fold, a NaN-input `fmod` becomes `frem nnan NaN, 1.0` which is **poison** by LLVM `nnan` semantics. Real miscompile.

## Reproducer

`opt -passes=instcombine -S repro.ll` → `ret double poison`. Expected: `ret double <some NaN>`.

The reproducer uses `llvm.assume(fcmp uno x, 0)` to teach InstCombine that `x` is NaN, while also asserting `x` is not `+Inf` or `-Inf`. Together these match the path that sets `IsNoNan=true` while the actual input is NaN.

## Severity

Real Alive2-falsifiable miscompile in default `-O2` (`optimizeFMod` runs in InstCombine for libc `fmod`). Defined-NaN behavior turned into poison.

## Fix

Either rename `IsNoNan` → `IsNoErrno` and gate `setHasNoNaNs(true)` on actual no-NaN evidence (`CI->hasNoNaNs() || (Known0.isKnownNeverNaN() && Known1.isKnownNeverNaN())`), or drop the `setHasNoNaNs(true)` call entirely.
