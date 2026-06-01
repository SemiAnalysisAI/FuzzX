target triple = "nvptx64-nvidia-cuda"

; Kernel: receives <8 x i1>, stores element 7 to output.
define ptx_kernel void @kern(<8 x i1> %a, ptr %out) {
  %e7 = extractelement <8 x i1> %a, i32 7
  %z7 = zext i1 %e7 to i32
  store i32 %z7, ptr %out
  ret void
}
