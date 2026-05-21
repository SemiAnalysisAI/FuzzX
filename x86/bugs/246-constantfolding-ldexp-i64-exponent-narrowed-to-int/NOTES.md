# 246 — ConstantFolding `llvm.ldexp.f64.i64` silently narrows i64 exponent to `int`

Component: `llvm/lib/Analysis/ConstantFolding.cpp` lines ~3715-3719

`scalbn(Op1V, Op2C->getSExtValue(), ...)` calls implicitly narrow `int64_t → int` via `scalbn`'s `int` parameter. The fold doesn't check whether the i64 exponent fits in `int` before calling. For exponents that wrap from large positive i64 to small `int`, the fold computes a finite small value instead of saturating to `+inf` (or `0`).

## Reproducer

```ll
%r = call double @llvm.ldexp.f64.i64(double 1.0, i64 4294967330)
```

(4294967330 = 2^32 + 34; narrowed to `int` becomes 34.)

`opt -passes=instcombine -S` → `ret double f0x4210000000000000` (i.e., `2^34 ≈ 1.71e10`).

Per LangRef: "result overflows → infinity with the same sign". Expected: `ret double +inf` (1.0 × 2^4294967330 vastly exceeds double range).

## Severity

Real Alive2-falsifiable miscompile in default `-O2`. Distinct from #195 (which is the chained-ldexp wrap); this one is the direct i64-exponent narrowing.

## Fix

Before calling `scalbn`, check `Op2C->getSExtValue() > std::numeric_limits<int>::max()` → return `+/-inf` based on sign of `Op1V`; check `< std::numeric_limits<int>::min()` → return `+/-0`.
