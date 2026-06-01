target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern(ptr byval(%struct.S) align 8 %p) {
entry:
  %old = atomicrmw add ptr %p, i32 1 seq_cst
  ret void
}
