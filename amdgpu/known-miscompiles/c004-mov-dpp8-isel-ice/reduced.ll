; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
entry:
  %r = call i32 @llvm.amdgcn.mov.dpp8.i32(i32 0, i32 0)
  store i32 %r, ptr addrspace(1) %out, align 4
  ret void
}

declare i32 @llvm.amdgcn.mov.dpp8.i32(i32, i32 immarg)

attributes #0 = { convergent nounwind "target-cpu"="gfx950" }
