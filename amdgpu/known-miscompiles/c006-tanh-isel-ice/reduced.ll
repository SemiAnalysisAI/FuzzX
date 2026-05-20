; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
entry:
  %r = call float @llvm.amdgcn.tanh.f32(float 0.0)
  store float %r, ptr addrspace(1) %out, align 4
  ret void
}

declare float @llvm.amdgcn.tanh.f32(float)

attributes #0 = { convergent nounwind "target-cpu"="gfx950" }
