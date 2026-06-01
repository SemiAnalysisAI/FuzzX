target triple = "nvptx64-nvidia-cuda"

; Callee: reads two consecutive i8 variadic args, returns the second.
; The second i8 should come from the byte the caller placed at offset 1,
; but the backend advances va_list by the promoted i16 alloc size (2),
; so it reads the second arg from offset 2 instead.
define i8 @second_i8(ptr %ap) {
  %first  = va_arg ptr %ap, i8
  %second = va_arg ptr %ap, i8
  ret i8 %second
}

; Matching caller packs the two i8 args at offsets 0 and 1 (1-byte stride).
declare i32 @variadic(i32, ...)
define i32 @caller(i8 %a, i8 %b) {
  %r = call i32 (i32, ...) @variadic(i32 0, i8 %a, i8 %b)
  ret i32 %r
}
