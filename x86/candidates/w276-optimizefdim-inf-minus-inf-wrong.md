# w276: optimizeFdim folds `fdim(+Inf,+Inf)` to qNaN instead of +0

**Severity:** Miscompile (wrong constant fold of a `memory(none)` libcall).

**Where:** `llvm/lib/Transforms/Utils/SimplifyLibCalls.cpp:3179-3203`
(file path: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Transforms/Utils/SimplifyLibCalls.cpp`)

## Root cause

`optimizeFdim` constant-folds `fdim(X, Y)` as:

```cpp
3197:  APFloat Difference = *X;
3198:  Difference.subtract(*Y, RoundingMode::NearestTiesToEven);
3199:
3200:  APFloat MaxVal =
3201:      maximum(Difference, APFloat::getZero(CI->getType()->getFltSemantics()));
3202:  return ConstantFP::get(CI->getType(), MaxVal);
```

The implementation evaluates `max(X - Y, +0)`. This is *almost* the textbook
definition of `fdim`, but the C standard (C99 7.12.12.1 / C23 7.12.12.1) is
phrased as a **comparison**, not a subtraction:

> The fdim functions determine the positive difference between their
> arguments: `x − y` if `x > y`, `+0` otherwise.

With both arguments equal to `+Inf` (or both equal to `-Inf`), the comparison
`x > y` is false, so the answer must be `+0`. But the subtraction
`+Inf - +Inf` is `qNaN`, and `maximum(qNaN, +0)` propagates the NaN (per IEEE
754‑2019 `maximum`, which is exactly what `llvm::maximum(APFloat,APFloat)`
implements — see `llvm/include/llvm/ADT/APFloat.h`).

So the optimizer folds `fdim(+Inf, +Inf)` and `fdim(-Inf, -Inf)` to `qNaN`,
whereas a correct implementation (e.g. glibc, musl) returns `+0`.

This is a constant fold of a libcall declared `memory(none)`, so there is no
errno / observable‑side‑effect escape hatch. The libcall is replaced with a
literal `qNaN` IR constant.

## Reproducer

```ll
; opt -passes=instcombine -S
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare double @fdim(double, double) memory(none)

; C standard: fdim(+Inf, +Inf) = +0.0
define double @test_inf_inf() {
  %r = call double @fdim(double 0x7FF0000000000000, double 0x7FF0000000000000)
  ret double %r
}

; C standard: fdim(-Inf, -Inf) = +0.0
define double @test_ninf_ninf() {
  %r = call double @fdim(double 0xFFF0000000000000, double 0xFFF0000000000000)
  ret double %r
}
```

**After `opt -passes='instcombine<no-verify-fixpoint>' -S`:**

```ll
define double @test_inf_inf() {
  ret double +qnan
}
define double @test_ninf_ninf() {
  ret double +qnan
}
```

Both should be `0.000000e+00`. Cross‑check at the same `opt` invocation:
`fdim(+Inf, -Inf)` folds to `+inf` (correct) and `fdim(-Inf, +Inf)` folds to
`0.000000e+00` (correct), confirming that only the equal‑infinities cases are
wrong.

## Suggested fix

Special‑case the equal‑infinities path before the subtraction, or fall back to
a comparison‑based definition:

```cpp
  // C: fdim(x,y) = x > y ? x - y : +0  (with NaN propagation)
  if (X->isNaN() || Y->isNaN())
    return ConstantFP::get(CI->getType(), APFloat::getQNaN(...));
  if (X->compare(*Y) != APFloat::cmpGreaterThan)
    return ConstantFP::get(CI->getType(), APFloat::getZero(...));
  APFloat Difference = *X;
  Difference.subtract(*Y, RoundingMode::NearestTiesToEven);
  return ConstantFP::get(CI->getType(), Difference);
```

(`compare` returns `cmpUnordered` on a NaN, so NaN propagation falls through
naturally to the explicit NaN check.)

## Default x86 -O2 only

Reproduces with `opt -O2 -S` on `x86_64-unknown-linux-gnu`; no source-level
changes required. The `memory(none)` attribute on the declaration is necessary
for `optimizeFdim` to fire (see line 3181‑3182 guard).
