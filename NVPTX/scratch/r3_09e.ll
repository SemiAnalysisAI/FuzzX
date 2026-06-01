target triple = "nvptx64-nvidia-cuda"
; kernel with <4 x i1>, read element 3 (true bit lives in byte0 bit3)
define ptx_kernel void @kern4(<4 x i1> %a, ptr %out) {
  %e3 = extractelement <4 x i1> %a, i32 3
  %z3 = zext i1 %e3 to i32
  store i32 %z3, ptr %out
  ret void
}
@g4 = global <4 x i1> zeroinitializer
@g2 = global <2 x i1> zeroinitializer
