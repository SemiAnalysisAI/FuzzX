# 233 — ConstantFolding folds `llvm.fmuladd` as fused FMA, but x86 default backend lowers it unfused — same source IR, two results

Component: `llvm/lib/Analysis/ConstantFolding.cpp` lines ~4102-4104 (constrained) and ~4125-4129 (non-constrained `llvm.fmuladd`)

Both code paths bucket `llvm.fmuladd` together with `llvm.fma` and call `APFloat::fusedMultiplyAdd` (a single-rounding fused operation). But per LangRef:

> `llvm.fmuladd` "is unspecified whether rounding will be performed between the multiplication and addition steps. Fusion is not guaranteed, even if the target platform supports it."

x86 default lowering (no `+fma`) emits `mulsd; addsd` — two roundings. So the constant fold result and the runtime result of the same `llvm.fmuladd` differ.

## Reproducer

```ll
%r = call double @llvm.fmuladd.f64(double 0x3FF0000000000001, double 0x3FF0000000000001, double 0xBFF0000000000002)
```

(operands: `1+2^-52`, `1+2^-52`, `-(1+2^-51)`)

- `opt -O2 -S` constant-folds to `ret double 0x3970000000000000` (i.e., `2^-104` — FMA result).
- At runtime on x86 without `+fma`, the same IR (through a non-foldable variable path) computes `mulsd → 1+2^-51 (rounded)`, then `addsd → 0.0`.

Same IR, two values: `2^-104` (fold) vs `0.0` (runtime). Real miscompile per Alive2-style refinement (constant fold should not commit to a result the runtime won't produce).

## Severity

Default x86 -O2. Bites any FP code using `__builtin_fmaf` / `__builtin_fma` on a non-FMA-supporting target.

## Fix

In the ConstantFolding paths, only fuse `llvm.fma` (the strict fused intrinsic). For `llvm.fmuladd`, either emit two separate roundings (matching default lowering) or refuse to constant-fold.
