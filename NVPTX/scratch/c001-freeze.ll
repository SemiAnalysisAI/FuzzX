target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern(ptr byval(%struct.S) align 8 %p, ptr addrspace(1) %out) {
entry:
  %f = freeze ptr %p
  %v = ptrtoint ptr %f to i64
  %t = trunc i64 %v to i32
  store i32 %t, ptr addrspace(1) %out, align 4
  ret void
}
