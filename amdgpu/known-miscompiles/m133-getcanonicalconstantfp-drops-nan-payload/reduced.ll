; Reproduces NaN payload loss in getCanonicalConstantFP
; (SIISelLowering.cpp:15820-15854).  Sibling shape to m118/m115/m124
; (canonicalize family) and m107/m110/m111/m120/m127/m128/m129 (NaN
; sign family).
;
; SNaN-quietening path at line 15841:
;
;   if (C.isSignaling()) {
;     // FIXME: Is this supposed to preserve payload bits?
;     return DAG.getConstantFP(CanonicalQNaN, SL, VT);
;   }
;
; AMDGPU HW `v_max_f32(SNaN, SNaN)` quiets by setting bit 22 only and
; preserves the rest of the payload.  The constant-fold drops ALL
; payload bits and returns the default-payload QNaN.
;
; Symmetric defect at line 15848-15849 for QNaN with non-default
; payload: HW preserves QNaN payload; constant-fold clobbers it to
; default.
;
; Test value: x = SNaN 0x7F8A5A5A (payload 0x0a5a5a).
;   Expected (HW v_max_f32): 0x7FCA5A5A (quiet bit set, payload kept)
;   Observed (const fold):   0x7FC00000 (default-payload QNaN)
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m133-getcanonicalconstantfp-drops-nan-payload/reduced.ll

source_filename = "m133-getcanonicalconstantfp-drops-nan-payload"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.canonicalize.f32(float)

define amdgpu_kernel void @snan_payload(ptr addrspace(1) %out) {
  ; SNaN 0x7F8A5A5A with payload 0x0a5a5a.
  ; Expected canonical: 0x7FCA5A5A (HW v_max sets bit 22, preserves rest).
  ; Observed: 0x7FC00000 (constant-fold gives default-payload QNaN).
  %c = call float @llvm.canonicalize.f32(float bitcast (i32 2139502170 to float))
  store float %c, ptr addrspace(1) %out
  ret void
}

define amdgpu_kernel void @qnan_payload(ptr addrspace(1) %out) {
  ; QNaN with non-default payload 0x7FCDEF12.
  ; Expected: 0x7FCDEF12 (HW preserves QNaN payload).
  ; Observed: 0x7FC00000 (constant-fold clobbers to canonical).
  %c = call float @llvm.canonicalize.f32(float bitcast (i32 2143092498 to float))
  store float %c, ptr addrspace(1) %out
  ret void
}
