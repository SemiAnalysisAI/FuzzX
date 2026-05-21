; m147: performClampCombine constant-fold returns sNaN bit-pattern
; unchanged; HW v_max_f32(sNaN, sNaN) (the lowering of CLAMP src,src)
; with IEEE_MODE=1 (default compute on gfx950) quiets the sNaN.
;
; SIISelLowering.cpp:18284-18303 (`performClampCombine`):
;
;   if (F < Zero || (F.isNaN() && ...DX10Clamp)) return +0;
;   if (F > One)                                  return 1.0;
;   return SDValue(CSrc, 0);   // <-- sNaN passes through
;
; When DX10Clamp is OFF (default for non-graphics kernels on gfx950),
; the F.isNaN() guard does not fire and the sNaN bit-pattern is
; returned unchanged.  HW would have quieted the sNaN by setting
; bit 22 of the mantissa.
;
; Reproducer: construct AMDGPUISD::CLAMP via the fmed3(c, 0.0, 1.0)
; pattern with c = sNaN.  performFPMed3ImmCombine recognises the
; pattern at SIISelLowering.cpp:16057, emits CLAMP(c), which
; performClampCombine then constant-folds.

source_filename = "m147-performclampcombine-drops-snan-quietening"
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.amdgcn.fmed3.f32(float, float, float)

define amdgpu_kernel void @t(ptr addrspace(1) %p) {
  ; sNaN constant: f32 0x7F800001 = sNaN with payload 1, sign=+
  %s = call float @llvm.amdgcn.fmed3.f32(
      float bitcast (i32 2139095041 to float),    ; sNaN payload=1
      float 0.0,
      float 1.0)
  store float %s, ptr addrspace(1) %p, align 4
  ret void
}
