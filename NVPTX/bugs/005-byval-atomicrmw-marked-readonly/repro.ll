target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern(ptr byval(%struct.S) align 8 %p, i1 %c) {
entry:
  %sel = select i1 %c, ptr %p, ptr %p
  %old = atomicrmw add ptr %sel, i32 7 seq_cst
  ret void
}
