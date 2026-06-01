target triple = "nvptx64-nvidia-cuda"

; Callee reads i8 then i32. The i32 va_arg has alignment 4, so the
; re-alignment masks the off-by-one and recovers the correct offset.
define i32 @i8_then_i32(ptr %ap) {
  %first  = va_arg ptr %ap, i8
  %second = va_arg ptr %ap, i32
  ret i32 %second
}

declare i32 @variadic(i32, ...)
define i32 @caller2(i8 %a, i32 %b) {
  %r = call i32 (i32, ...) @variadic(i32 0, i8 %a, i32 %b)
  ret i32 %r
}
