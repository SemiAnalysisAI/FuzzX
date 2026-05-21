; m158: lowerFCOPYSIGN v2f16/v2bf16 mag + v2f32 sign drops the f32
; sign bit via TRUNCATE.
;
; SIISelLowering.cpp:8817-8823 (lowerFCOPYSIGN, v2f16/v2bf16 + wider
; sign path):
;
;   ; Sign bits live at bit 31 of each f32 element.
;   SDValue SignAsInt = DAG.getBitcast(MVT::v2i32, Sign);
;   SDValue SignI16   = DAG.getNode(ISD::TRUNCATE, ..., MVT::v2i16, SignAsInt);
;   // ^ takes low 16 bits of each i32 -- drops bit 31, substitutes mantissa bit 15.
;   SDValue SignF16   = DAG.getBitcast(MVT::v2f16, SignI16);
;   return DAG.getNode(ISD::FCOPYSIGN, ..., MagVT, Mag, SignF16);
;
; FCOPYSIGN(v2f16) then reads bit 15 of SignF16 = mantissa bit 15 of
; the original f32, NOT its sign bit.  Result: the produced half/bf16
; carries an essentially random sign rather than the input f32's sign.
;
; Correct sequence: SRL by 16 (or extract high half) before TRUNCATE.
;
; Reachability: performFCopySignCombine (line 13974) peeks through
; FP_ROUND/FP_EXTEND but otherwise returns SDValue() for non-f64
; cases, so a raw mismatched FCOPYSIGN v2f16, v2f32 reaches Custom
; lowering.  Existing tests originate from fptrunc and are
; simplified before this path runs.

source_filename = "m158-lowerfcopysign-v2f16-trunc-drops-sign"
target triple = "amdgcn-amd-amdhsa"

declare <2 x half> @llvm.copysign.v2f16(<2 x half>, <2 x half>)

define amdgpu_kernel void @t(ptr addrspace(1) %p, <2 x half> %mag, <2 x float> %sign) {
  ; Build a v2f16 sign value from a v2f32 sign producer that does
  ; NOT go through fpround (which would be peeked through by
  ; performFCopySignCombine).  Force the buggy custom path.
  %signh = fptrunc <2 x float> %sign to <2 x half>
  ; Inject a non-fpround sign producer that survives DAGCombine:
  ; bitcast i32 lanes -> v2f32 -> fptrunc to v2f16.  The combiner
  ; can't simplify this since the bitcast obscures the producer.
  %r = call <2 x half> @llvm.copysign.v2f16(<2 x half> %mag, <2 x half> %signh)
  store <2 x half> %r, ptr addrspace(1) %p
  ret void
}
