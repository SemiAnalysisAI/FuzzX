; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release
; RUN-MCPU: gfx950
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out) #0 {
entry:
  %x = load i32, ptr addrspace(1) %in, align 4
  %a0 = and i32 %x, 31
  %b0 = and i32 %x, 15
  %a = uitofp i32 %a0 to float
  %b = uitofp i32 %b0 to float
  %r = call float @llvm.amdgcn.fma.legacy(float %a, float %b, float 3.000000e+00)
  %i = fptoui float %r to i32
  store i32 %i, ptr addrspace(1) %out, align 4
  ret void
}

declare float @llvm.amdgcn.fma.legacy(float, float, float)

attributes #0 = { nounwind "target-cpu"="gfx950" }
