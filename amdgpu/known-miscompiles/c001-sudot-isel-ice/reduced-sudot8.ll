; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release
; RUN-MCPU: gfx950
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
entry:
  %r = call i32 @llvm.amdgcn.sudot8(i1 true, i32 0, i1 true, i32 0, i32 0, i1 false)
  store i32 %r, ptr addrspace(1) %out, align 4
  ret void
}

declare i32 @llvm.amdgcn.sudot8(i1 immarg, i32, i1 immarg, i32, i32, i1 immarg)

attributes #0 = { nounwind "target-cpu"="gfx950" }
