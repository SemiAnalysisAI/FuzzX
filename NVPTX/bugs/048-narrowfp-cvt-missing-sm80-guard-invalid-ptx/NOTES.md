# 048 — nvvm ff2bf16x2 (rn/rz, [relu]) intrinsics emit cvt.*.bf16x2.f32 below sm_80 — invalid PTX

- **Kind:** other (invalid PTX)
- **Reachable via:** llc -mcpu<sm_80
- **Component:** NVPTXIntrinsics.td 2099-2102  (round-7 area `P03-cvt-narrowfp-arch`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration (sibling guards/orderings); no local `ptxas` was available to execute the rejection.

## Summary

narrow-fp cvt intrinsics (`f2bf16`/`ff2bf16x2`/`f2f16`/`ff2f16x2`, incl `.relu`) use standalone Pats lacking the instruction's sm_80 guard → `cvt.*.bf16/.bf16x2/.relu.f16` emitted below sm_80

## Mechanism / root cause

The four standalone patterns
  def : Pat<(int_nvvm_ff2bf16x2_rn f32:$a, f32:$b),      (CVT_bf16x2_f32 $a, $b, CvtRN)>;
  def : Pat<(int_nvvm_ff2bf16x2_rn_relu ...), (CVT_bf16x2_f32 ... CvtRN_RELU)>;
  def : Pat<(int_nvvm_ff2bf16x2_rz ...),      (CVT_bf16x2_f32 ... CvtRZ)>;
  def : Pat<(int_nvvm_ff2bf16x2_rz_relu ...), (CVT_bf16x2_f32 ... CvtRZ_RELU)>;
have NO pattern-level Predicates/Requires. The target instruction CVT_bf16x2_f32 (NVPTXInstrInfo.td CVT_FROM_FLOAT_V2_SM80, line 626-630) carries Requires<[hasPTX<70>, hasSM<80>]>, but for a *standalone* `def : Pat` the instruction's Requires is NOT propagated to the match — only the Pat's own predicates gate selection (empirically confirmed: a CVT_bf16x2_f32 pattern fires at sm_53). Per the PTX ISA, .bf16/.bf16x2 and `cvt` to them require .target sm_80 (PTX ISA 7.0); ptxas rejects with "Feature '.bf16' requires .target sm_80 or higher" / "Feature 'cvt with .f32.bf16' requires .target sm_80 or higher". The LLVM-original patch (reviews.llvm.org/D116673) documents these exact conversions as Requires<[hasPTX70,hasSM80]>. The sibling satfinite patterns (lines 2103-2108) correctly wrap a `let Predicates=[hasPTX<81>,hasSM<80>]`, and the build_vector->CVT_bf16x2_f32 pattern (NVPTXInstrInfo.td:860-863) is correctly guarded sm_80 — proving the maintainers intend an sm_80 guard that is missing here. The `.relu` saturation modifier itself is also an sm_80/PTX7.0 feature (RELU_FLAG added by D116673).

## Trigger

Call llvm.nvvm.ff2bf16x2.rn / .rn.relu / .rz / .rz.relu on any -mcpu below sm_80 (e.g. sm_53, sm_70, sm_75).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
declare <2 x bfloat> @llvm.nvvm.ff2bf16x2.rn(float, float)
define <2 x bfloat> @f(float %a, float %b) {
  %r = call <2 x bfloat> @llvm.nvvm.ff2bf16x2.rn(float %a, float %b)
  ret <2 x bfloat> %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_53 -o - repro.ll`

## Related instances (same root cause — standalone Pat drops the instruction Requires)
- `int_nvvm_ff2bf16x2_*` (NVPTXIntrinsics.td 2099-2102) → `cvt.rn.bf16x2.f32`
- `int_nvvm_f2bf16_*` (2141-2144) → `cvt.rn.bf16.f32`
- `int_nvvm_ff2f16x2_*` (2120-2123) → `cvt.rn.relu.f16x2.f32`
- `int_nvvm_f2f16_*_relu` (2153,2155) → `cvt.rn.relu.f16.f32`
All emitted on sm_53 under .version 4.2; the sibling satfinite/build_vector patterns are correctly guarded `[hasPTX<70>,hasSM<80>]`.

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.95).
