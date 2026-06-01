target triple = "nvptx64-unknown-cuda"

define ptx_kernel void @nvload(ptr noalias readonly %a, ptr %out) {
  %ag = addrspacecast ptr %a to ptr addrspace(1)
  %v = load i32, ptr addrspace(1) %ag, align 4
  store i32 %v, ptr %out, align 4
  ret void
}
