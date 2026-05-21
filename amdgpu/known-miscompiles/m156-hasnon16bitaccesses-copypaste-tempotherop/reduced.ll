; m156: `hasNon16BitAccesses` copy-paste bug -- OpIs16Bit check uses
; TempOtherOp.getValueSizeInBits() instead of TempOp.getValueSizeInBits().
;
; SIISelLowering.cpp:14923-14924:
;
;   auto OpIs16Bit =
;       TempOtherOp.getValueSizeInBits() == 16 || isExtendedFrom16Bits(TempOp);
;                ^^^^^^^^^^^^ should be TempOp.getValueSizeInBits()
;
; Two lines down the symmetric `OtherOpIs16Bit` clause correctly uses
; TempOtherOp on both sides, making this an obvious copy-paste error.
;
; Callsite: performOrCombine -> matchPERM at SIISelLowering.cpp:15070.
; Used to decide whether to lower `or (zext i16, ...)`-style patterns
; into v_perm_b32 (a byte-perm) vs keep 16-bit ops.
;
; Symptom:
;   - When Op is 32-bit and OtherOp is 16-bit: OpIs16Bit becomes
;     spuriously true.  Combine concludes "both are 16-bit", skips
;     v_perm, and may leave a 16-bit-shape codegen for a value that
;     isn't actually 16-bit -- with zext semantics, can drop upper
;     16 bits of Op.
;   - When OtherOp is 32-bit and Op is 16-bit/extended: OpIs16Bit
;     forced false, function returns true, v_perm always selected
;     (lost 16-bit optimization opportunity).
;
; This reproducer constructs `or (zext i16 -> i32) %a32` -- the
; mixed-width or-tree that triggers matchPERM's hasNon16BitAccesses
; gate.

source_filename = "m156-hasnon16bitaccesses-copypaste-tempotherop"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @t(ptr addrspace(1) %in, ptr addrspace(1) %out) {
  %xi = load i32, ptr addrspace(1) %in, align 4
  ; OtherOp: 16-bit zext.
  %h  = trunc i32 %xi to i16
  %z  = zext i16 %h to i32
  ; Op: 32-bit.  The or-tree feeds matchPERM through performOrCombine.
  %m  = and i32 %xi, 65280              ; mask out byte 1 of xi
  %s  = shl  i32 %m, 8                  ; shift up
  %p  = or i32 %z, %s
  store i32 %p, ptr addrspace(1) %out, align 4
  ret void
}
