; RUN-INPUTS: 0x00000000
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release

target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 0
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 0

  %cond = icmp slt i32 %n, 0
  %x = select i1 %cond, i32 %n, i32 0
  %a = and i32 %x, 1023
  %b = and i32 %v, 1023
  %af = uitofp i32 %a to float
  %bf = uitofp i32 %b to float
  %product = fmul float %af, %bf
  %product.i = fptoui float %product to i32

  %same = icmp ne i32 %a, %v
  %same.i = zext i1 %same to i32
  %sub = sub i32 %v, %same.i
  %sub.masked = and i32 %sub, 1023
  %product.masked = and i32 %product.i, 1023
  %sub.f = uitofp i32 %sub.masked to float
  %product.f = uitofp i32 %product.masked to float
  %sub.d = fpext float %sub.f to double
  %product.d = fpext float %product.f to double
  %sum.d = fadd double %sub.d, %product.d
  %sum.f = fptrunc double %sum.d to float
  %sum.i = fptoui float %sum.f to i32
  %result = add i32 %sum.i, %product.i
  store i32 %result, ptr addrspace(1) %out.ptr, align 4
  ret void
}

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
