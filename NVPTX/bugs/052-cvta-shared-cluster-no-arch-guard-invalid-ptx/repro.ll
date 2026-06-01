target triple = "nvptx64-nvidia-cuda"
define ptr @cast_cluster_to_generic(ptr addrspace(7) %p) {
  %c = addrspacecast ptr addrspace(7) %p to ptr
  ret ptr %c
}
define ptr addrspace(7) @cast_generic_to_cluster(ptr %p) {
  %c = addrspacecast ptr %p to ptr addrspace(7)
  ret ptr addrspace(7) %c
}
