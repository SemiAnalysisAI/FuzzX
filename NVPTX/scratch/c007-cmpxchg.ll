target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern_cas(ptr byval(%struct.S) align 8 %p, i1 %c) {
entry:
  %sel = select i1 %c, ptr %p, ptr %p
  %r = cmpxchg ptr %sel, i32 0, i32 7 seq_cst seq_cst
  ret void
}
