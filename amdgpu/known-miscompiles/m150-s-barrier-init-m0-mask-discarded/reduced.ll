; m150: SIISelLowering.cpp:12450-12459 - M0 packing for
; amdgcn.s.barrier.init / amdgcn.s.barrier.signal.var discards the
; mask immediately after computing it.
;
; The intent (per the comments at lines 12448-12449): pack BarrierID
; into M0[5:0] and member-count into M0[21:16].  The code:
;
;   M0Val = SDValue(DAG.getMachineNode(AMDGPU::S_AND_B32, ..., CntOp,
;                                      getTargetConstant(0x3F, ...)), 0);  // (1)
;   constexpr unsigned ShAmt = 16;
;   M0Val = DAG.getNode(ISD::SHL, DL, MVT::i32, CntOp,                     // (2)
;                       getShiftAmountConstant(ShAmt, MVT::i32, DL));
;
; Line (1) computes (CntOp & 0x3F) and assigns to M0Val, then line
; (2) immediately OVERWRITES M0Val with (CntOp << 16) using the
; UNMASKED CntOp.  The intended `(CntOp & 0x3F) << 16` is never
; produced.
;
; If the user passes a member-count with bits >=6 set (e.g. 0x40,
; 0x100, etc.), those bits land in M0[27:22] -- corrupting the
; barrier ID / unrelated M0 fields when the HW decodes M0.
;
; The bug is in the SDAG lowering, so it is independent of which
; concrete target ultimately consumes the synthesized M0 value.  The
; intrinsic is currently used by gfx12+ split-barriers, but the
; defect lives in the lowering shared with future gfx950+ revisions.
;
; This reproducer triggers the buggy lowering by calling
; amdgcn.s.barrier.init with a non-zero member-count and inspecting
; the resulting M0 setup in asm.

source_filename = "m150-s-barrier-init-m0-mask-discarded"
target triple = "amdgcn-amd-amdhsa"

declare void @llvm.amdgcn.s.barrier.init(i32, i32)

define amdgpu_kernel void @t(i32 %cnt) {
  ; CntOp = 0x40 -> bit 6 set (out of mask).
  ; Buggy lowering: M0 = (0x40 << 16) = 0x400000  -> M0[22] = 1 (corrupt)
  ; Intended:        M0 = ((0x40 & 0x3F) << 16) = 0  (just barrier ID)
  call void @llvm.amdgcn.s.barrier.init(i32 0, i32 %cnt)
  ret void
}
