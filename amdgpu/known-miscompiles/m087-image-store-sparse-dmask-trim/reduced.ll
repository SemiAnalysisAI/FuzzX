; Test: image_store with sparse DMask 0b1010 (Y and W), vdata = <a, 0>
; Expected without simplification: writes Y=a, W=0
; Bug: trimTrailingZerosInVector drops trailing zero, transforms to DMask=0b0010
;      which only writes Y, leaving W untouched.

target triple = "amdgcn-amd-amdpal"

declare void @llvm.amdgcn.image.store.1d.v2f32.i32.v8i32(<2 x float>, i32 immarg, i32, <8 x i32>, i32 immarg, i32 immarg)

define amdgpu_ps void @sparse_dmask_trailing_zero(<8 x i32> inreg %rsrc, float %a, i32 %s) {
  %v0 = insertelement <2 x float> poison, float %a, i32 0
  %v1 = insertelement <2 x float> %v0, float 0.0, i32 1
  ; DMask = 0b1010 = 10 -- write Y and W only
  call void @llvm.amdgcn.image.store.1d.v2f32.i32.v8i32(<2 x float> %v1, i32 10, i32 %s, <8 x i32> %rsrc, i32 0, i32 0)
  ret void
}
