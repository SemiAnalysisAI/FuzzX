target triple = "nvptx64-nvidia-cuda"

define ptr @f(ptr addrspace(101) %p) {
  %g = addrspacecast ptr addrspace(101) %p to ptr
  ret ptr %g
}
