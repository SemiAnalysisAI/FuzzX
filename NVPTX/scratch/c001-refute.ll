target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern(ptr byval(%struct.S) align 8 %p, ptr addrspace(1) %out) {
entry:
  %c = icmp eq ptr %p, null
  %z = zext i1 %c to i32
  store i32 %z, ptr addrspace(1) %out, align 4
  ret void
}
