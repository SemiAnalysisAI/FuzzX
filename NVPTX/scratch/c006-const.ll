target triple = "nvptx64-nvidia-cuda"

; Constant-folded check: shift = 0x100000005 = 4294967301, x = 1
; icmp ult 4294967301, 64 is FALSE -> select returns 0. shl is poison (unselected).
define i64 @const_shl_i64() {
  %cmp = icmp ult i64 4294967301, 64
  %shl = shl i64 1, 4294967301
  %sel = select i1 %cmp, i64 %shl, i64 0
  ret i64 %sel
}

; srl ugt: x = -1, shift = 0x100000005. icmp ugt 4294967301, 63 TRUE -> select returns 0.
define i64 @const_srl_i64_ugt() {
  %cmp = icmp ugt i64 4294967301, 63
  %shr = lshr i64 -1, 4294967301
  %sel = select i1 %cmp, i64 0, i64 %shr
  ret i64 %sel
}
