target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern(ptr byval(%struct.S) align 8 %p, i1 %c) {
entry:
  %sel = select i1 %c, ptr %p, ptr %p
  store i32 7, ptr %sel, align 4
  ret void
}
