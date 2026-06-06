# 040 - `ldexp(x, i64 K)` multiply fold truncates exponent to `int`

Component: `llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp:3227`

InstCombine rewrites `ldexp(x, K)` to `fmul x, 2^K` when the exponent is a
constant. The fold computes the scale factor with:

```cpp
scalbn(APFloat::getOne(FPTy), static_cast<int>(ConstExp), ...)
```

For an `i64` exponent, this truncates the exponent to `int`. In this
reproducer, `4294967330 == 2^32 + 34`, so InstCombine computes `2^34` and
emits:

```llvm
%r = fmul double %x, f0x4210000000000000
```

Witness: for `%x = 1.0`, source `llvm.ldexp.f64.i64(1.0, 4294967330)`
overflows and should return `+inf` per LangRef. The optimized code returns the
finite value `2^34`.

This is distinct from the older chained-`ldexp` exponent-combination bug and
from the now-fixed constant-folding path in `ConstantFolding.cpp`. It uses the
ordinary `llvm.ldexp` intrinsic with no strictfp and no fast-math flags.

Verifier: Dirac the 2nd (019e9975-1144-7723-af2c-c79eb0bae4ec) returned YES
for this adjusted current repro.
