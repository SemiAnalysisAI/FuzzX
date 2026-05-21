; m142: SDAG image-intrinsic D16 detection misses bf16.
;
; SIISelLowering::lowerImage at lines 10190 (store) and 10203 (load)
; uses `getScalarType() == MVT::f16` to detect D16 dataset.  bf16 is
; a distinct MVT, so <N x bfloat> data silently:
;   - skips handleD16VData (load path: skips D16 reconstruction)
;   - selects the non-D16 MIMG opcode
;   - computes NumVDataDwords = ceil(bytes / 32) from the 16-bit-
;     element vector, producing wrong VReg width
; The HasD16 guard at 10191 is also bypassed -- no diagnostic.
;
; GISel correctly handles bf16 via getScalarType() == S16
; (AMDGPULegalizerInfo.cpp:7182) -- so GISel and SDAG diverge.
;
; Reproducer compiles cleanly via SDAG but emits a non-D16 image_sample
; with mismatched VReg width.  Run with:
;   llc -mtriple=amdgcn -mcpu=gfx950 -global-isel=0 -O2 reduced.ll
;
; And compare against -global-isel=1.

source_filename = "m142-image-d16-bf16-skipped"
target triple = "amdgcn-amd-amdhsa"

declare <4 x bfloat> @llvm.amdgcn.image.sample.2d.v4bf16.f32(i32, float, float, <8 x i32>, <4 x i32>, i1, i32, i32)

define amdgpu_ps <4 x bfloat> @t(<8 x i32> inreg %r, <4 x i32> inreg %s, float %x, float %y) {
  %v = call <4 x bfloat> @llvm.amdgcn.image.sample.2d.v4bf16.f32(
         i32 15, float %x, float %y, <8 x i32> %r, <4 x i32> %s,
         i1 0, i32 0, i32 0)
  ret <4 x bfloat> %v
}
