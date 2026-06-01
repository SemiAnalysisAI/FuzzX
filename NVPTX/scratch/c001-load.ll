target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern(ptr byval(%struct.S) align 8 %p, ptr addrspace(1) %out) {
entry:
  %v = load i32, ptr %p, align 8
  store i32 %v, ptr addrspace(1) %out, align 4
  ret void
}
