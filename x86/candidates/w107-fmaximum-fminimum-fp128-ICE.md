# X86: `llvm.maximum.f128` / `llvm.minimum.f128` ICE — "Cannot select setcc i128 seteq"

## Summary

`llvm.maximum.f128` and `llvm.minimum.f128` (scalar fp128) crash the X86 backend with "Cannot select" during isel. The reduction variants (`llvm.vector.reduce.fmaximum.vNf128`, `llvm.vector.reduce.fminimum.vNf128`) crash for the same root cause.

Generic `LegalizeDAG` expansion of FMAXIMUM/FMINIMUM emits a `setcc` on the i128 bitcast of the fp128 input to detect signed-zero / NaN edge cases. The resulting `seteq` on i128 is not supported by X86 isel patterns, so the legalizer leaves it for the DAG matcher which can't select it.

## Reproducer (minimal)

```llvm
; t48d.ll
define fp128 @t(fp128 %a, fp128 %b) {
  %r = call fp128 @llvm.maximum.f128(fp128 %a, fp128 %b)
  ret fp128 %r
}
declare fp128 @llvm.maximum.f128(fp128, fp128)
```

Identical crash with `llvm.minimum.f128`:

```llvm
define fp128 @t(fp128 %a, fp128 %b) {
  %r = call fp128 @llvm.minimum.f128(fp128 %a, fp128 %b)
  ret fp128 %r
}
declare fp128 @llvm.minimum.f128(fp128, fp128)
```

And via `vector.reduce.fmaximum.v2f128`:

```llvm
define fp128 @t(<2 x fp128> %v) {
  %r = call fp128 @llvm.vector.reduce.fmaximum.v2f128(<2 x fp128> %v)
  ret fp128 %r
}
declare fp128 @llvm.vector.reduce.fmaximum.v2f128(<2 x fp128>)
```

Reference: `llvm.maxnum.f128` / `llvm.minnum.f128` compile fine (handled via libcall path `RTLIB::FMAX_F128`). The "minimum/maximum" variants — which have stricter NaN/sign-zero semantics — were never given an f128 lowering path.

## Command

```
llc -O2 -mtriple=x86_64-linux-gnu t48d.ll -o -
```

No special features needed.

## Crash

```
LLVM ERROR: Cannot select: 0x...: i8 = setcc 0x..., Constant:i128<0>, seteq:ch
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ ...
Stack dump:
  llvm::report_fatal_error(llvm::Twine const&, bool)
  abort
```

For `minimum.f128` the constant differs (`Constant:i128<-170141183460469231731687303715884105728>` = `INT128_MIN` — the sign-bit mask used to detect signed zero).

## Root cause

`llvm/lib/Target/X86/X86ISelLowering.cpp` sets:

```cpp
setOperationAction(ISD::FMAXIMUM, MVT::f32, Custom);  // line ~1067
setOperationAction(ISD::FMINIMUM, MVT::f32, Custom);
// ... and for f64, f16/bf16 vector types ...
```

But never registers an action for `MVT::f128`. The default action (`Legal`) is wrong: there is no `FMAXIMUMf128` selection pattern. The DAGTypeLegalizer doesn't soften the op (no libcall is registered — there's no libm `fmaximumq` analog to dispatch to), and the generic expander rewrites it as a sequence containing `setcc <i128>, <i128>` that X86 isel cannot match.

Fix should be one of:
- Register `setOperationAction(ISD::FMAXIMUM, MVT::f128, LibCall)` (with a new RTLIB call, similar to the recently-added `FMAXIMUM_NUM_F128`).
- Or `Expand` via maxnum + NaN-fixup using `is.fpclass` (which has an fp128 path).
- Or generic expander needs to call `softenSetCCOperands` for the f128->i128 cast.
