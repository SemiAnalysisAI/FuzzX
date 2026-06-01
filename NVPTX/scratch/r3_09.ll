target triple = "nvptx64-nvidia-cuda"

; Kernel ABI miscompile: <8 x i1> param is declared as 1 byte (packed, 8 bits),
; matching getTypeAllocSize and the canonical in-memory/global layout, but the
; generated PTX reads element 7 from byte offset +7 of a 1-byte param slot.
; The host driver only ever populates the single declared byte, so byte +7 is
; out of bounds. For a defined host input (1 byte = 0x80 => only element 7 set),
; this returns garbage instead of 1.
define ptx_kernel void @kern(<8 x i1> %a, ptr %out) {
  %e7 = extractelement <8 x i1> %a, i32 7
  %z7 = zext i1 %e7 to i32
  store i32 %z7, ptr %out
  ret void
}

; Confirms canonical layout: <8 x i1> is 1 packed byte both in memory and as a global.
@g = global <8 x i1> zeroinitializer
