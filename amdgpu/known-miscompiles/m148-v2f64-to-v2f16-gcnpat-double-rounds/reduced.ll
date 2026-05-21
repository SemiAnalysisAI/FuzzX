; m148: v2f64 -> v2f16 GCNPat double-rounds (V_CVT_F32_F64 then
; V_CVT_PK_F16_F32); the scalar f64 -> f16 path single-rounds via
; LowerF64ToF16Safe.  Same IR, same gfx950: scalar `fptrunc double
; to half` vs lane-0 of `fptrunc <2 x double> to <2 x half>` differ
; by 1 ULP near half-way f16 boundaries.
;
; VOP3Instructions.td:1461-1463 (the GCNPat) emits the vector form
; without the inexact-to-odd correction that LowerF64ToF16Safe
; applies for the scalar form (AMDGPUISelLowering.cpp:3787-3873).
;
; For f64 values that sit on a half-way boundary between two f16
; values where the intermediate f32 falls on the "wrong" half, the
; vector path produces a different f16 from the scalar path.
;
; This reproducer stores both the scalar fptrunc result and the
; first lane of the vector fptrunc result side-by-side in one i32
; (low half = scalar, high half = vector lane 0) so the divergence
; is observable within a single kernel.

source_filename = "m148-v2f64-to-v2f16-gcnpat-double-rounds"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @t(ptr addrspace(1) %in, ptr addrspace(1) %out) {
  %xi = load i64, ptr addrspace(1) %in, align 8
  %xd = bitcast i64 %xi to double

  ; Scalar path: LowerF64ToF16Safe (single-rounding).
  %scalar_h = fptrunc double %xd to half

  ; Vector path: V_CVT_F32_F64 + V_CVT_PK_F16_F32 (double-rounding).
  %v = insertelement <2 x double> poison, double %xd, i32 0
  %v2 = insertelement <2 x double> %v,     double %xd, i32 1
  %vh = fptrunc <2 x double> %v2 to <2 x half>
  %vec_h = extractelement <2 x half> %vh, i32 0

  ; Pack both results into i32.
  %scalar_i = bitcast half %scalar_h to i16
  %vec_i    = bitcast half %vec_h    to i16
  %lo = zext i16 %scalar_i to i32
  %hi = zext i16 %vec_i    to i32
  %hi_s = shl i32 %hi, 16
  %combined = or i32 %hi_s, %lo
  store i32 %combined, ptr addrspace(1) %out, align 4
  ret void
}
