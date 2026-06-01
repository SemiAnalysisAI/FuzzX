target triple = "nvptx64-nvidia-cuda"

; A device function that takes an i8 view and bitcasts to <8 x i1> then passes it on.
; Caller path: pack 1 byte from memory -> store to param. Compare to in-memory layout.
define i32 @caller(ptr %p) {
  %v = load <8 x i1>, ptr %p          ; loads 1 byte, unpacks 8 bits
  %r = call i32 @callee(<8 x i1> %v)  ; passes vector
  ret i32 %r
}
define i32 @callee(<8 x i1> %a) {
  %e7 = extractelement <8 x i1> %a, i32 7
  %z7 = zext i1 %e7 to i32
  ret i32 %z7
}
