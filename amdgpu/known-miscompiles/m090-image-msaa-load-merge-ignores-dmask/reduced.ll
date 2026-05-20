; Test: two image_load_2dmsaa calls with the same coords but DIFFERENT DMasks
; get merged.  AMDGPUImageIntrinsicOptimizer's grouping loop starts arg
; comparison at index 1, never checking arg 0 (DMask), so both loads end up in
; the same group and the merge uses the *first* call's DMask for both.
;
; Concretely:
;   %a = image.load.2dmsaa(dmask=1 = R, ..., fragId=0)   ; reads R lane
;   %b = image.load.2dmsaa(dmask=8 = A, ..., fragId=1)   ; should read A lane
; After this pass, both extracts pull from a *single* image_msaa_load with
; dmask=1 (R), so %b ends up holding R for fragId=1 instead of A for fragId=1.

target triple = "amdgcn-amd-amdpal"

declare float @llvm.amdgcn.image.load.2dmsaa.f32.i32(i32 immarg, i32, i32, i32, <8 x i32>, i32 immarg, i32 immarg)

define amdgpu_ps <2 x float> @dmask_mismatch_merge(<8 x i32> inreg %rsrc, i32 %s, i32 %t) {
  ; First call: DMask = 0x1 (R), FragId = 0
  %a = call float @llvm.amdgcn.image.load.2dmsaa.f32.i32(i32 1, i32 %s, i32 %t, i32 0, <8 x i32> %rsrc, i32 0, i32 0)
  ; Second call: DMask = 0x8 (A), FragId = 1
  %b = call float @llvm.amdgcn.image.load.2dmsaa.f32.i32(i32 8, i32 %s, i32 %t, i32 1, <8 x i32> %rsrc, i32 0, i32 0)
  %v0 = insertelement <2 x float> poison, float %a, i32 0
  %v1 = insertelement <2 x float> %v0,    float %b, i32 1
  ret <2 x float> %v1
}
