target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(7) %p, ptr addrspace(1) %out) {
entry:
  %r = cmpxchg weak ptr addrspace(7) %p, i32 1, i32 2 monotonic monotonic
  %succ = extractvalue { i32, i1 } %r, 1
  %z = zext i1 %succ to i32
  store i32 %z, ptr addrspace(1) %out, align 4
  ret void
}
