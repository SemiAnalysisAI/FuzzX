target triple = "nvptx64-nvidia-cuda"

; IR semantics: when %shift >= 64 the icmp is false, so select returns 0
; (the poison shl is in the unselected arm). For %shift=0x100000005 -> result 0.
define i64 @guarded_shl_i64(i64 %x, i64 %shift) {
  %cmp = icmp ult i64 %shift, 64
  %shl = shl i64 %x, %shift
  %sel = select i1 %cmp, i64 %shl, i64 0
  ret i64 %sel
}

; IR semantics: when %shift > 63 the icmp is true, so select returns 0.
define i64 @guarded_srl_i64_ugt(i64 %x, i64 %shift) {
  %cmp = icmp ugt i64 %shift, 63
  %shr = lshr i64 %x, %shift
  %sel = select i1 %cmp, i64 0, i64 %shr
  ret i64 %sel
}
