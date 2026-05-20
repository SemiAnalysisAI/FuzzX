; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %gptr,
                                                 ptr addrspace(3) %lptr) #0 {
entry:
  call void @llvm.amdgcn.global.load.lds(ptr addrspace(1) %gptr,
                                         ptr addrspace(3) %lptr,
                                         i32 4, i32 0, i32 0)
  ret void
}

declare void @llvm.amdgcn.global.load.lds(ptr addrspace(1), ptr addrspace(3),
                                          i32 immarg, i32 immarg, i32 immarg)

attributes #0 = { convergent nounwind "target-cpu"="gfx950" }
