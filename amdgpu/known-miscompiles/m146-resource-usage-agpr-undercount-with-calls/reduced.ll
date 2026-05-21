; m146: AMDGPUResourceUsageAnalysis asymmetric AGPR/VGPR tracking in
; the call-path branch.
;
; AMDGPUResourceUsageAnalysis.cpp:173-177 (the fast-path init) sets
; NumExplicitSGPR and NumAGPR via getNumUsedPhysRegs(..., IncludeCalls=false).
; The per-MI call-path scan at lines 188-321 only updates VGPR
; (MaxVGPR), never NumAGPR or NumExplicitSGPR.
;
; On gfx950 (MAI/AGPR-heavy kernels with calls), AGPR usage can be
; under-reported relative to VGPR. Combined with unified VGPR+AGPR
; allocation on gfx90a/gfx950, downstream max(NumVGPR, NumAGPR) for
; occupancy computation may select the stale AGPR count, allowing
; over-allocation in the kernel descriptor.
;
; Reproducer (sketch): kernel that calls a callee writing AGPR ranges
; via MFMA between two MFMA-using regions. The pre-call MaxVGPR scan
; sees no AGPR writes; the post-call NumAGPR snapshot was taken once
; with IncludeCalls=false and never updated. Reported .agpr_count
; in the kernel descriptor is lower than the actual peak AGPR usage.
;
; Symptom: dispatch with high occupancy that the runtime treats as
; "enough AGPRs for two waves per CU" when actually only one wave can
; fit -> register spill at runtime / wave underutilization.

source_filename = "m146-resource-usage-agpr-undercount-with-calls"
target triple = "amdgcn-amd-amdhsa"

declare <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(float, float, <16 x float>, i32, i32, i32)
declare void @external_callee(<16 x float>)

define amdgpu_kernel void @t(ptr addrspace(1) %p) {
  %z = load <16 x float>, ptr addrspace(1) %p
  %r1 = call <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(
      float 1.0, float 1.0, <16 x float> %z, i32 0, i32 0, i32 0)
  ; Call between MFMA: callee may use AGPRs that the pre-call snapshot
  ; doesn't see, the post-call scan doesn't recount AGPRs.
  call void @external_callee(<16 x float> %r1)
  %r2 = call <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(
      float 1.0, float 1.0, <16 x float> %r1, i32 0, i32 0, i32 0)
  store <16 x float> %r2, ptr addrspace(1) %p
  ret void
}
