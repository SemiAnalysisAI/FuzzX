target triple = "nvptx64-nvidia-cuda"

; Callee: receives <8 x i1>, returns elements 0 and 7 OR'd into i32.
define i32 @callee(<8 x i1> %a) {
  %e0 = extractelement <8 x i1> %a, i32 0
  %e7 = extractelement <8 x i1> %a, i32 7
  %z0 = zext i1 %e0 to i32
  %z7 = zext i1 %e7 to i32
  %s = shl i32 %z7, 1
  %r = or i32 %z0, %s
  ret i32 %r
}
