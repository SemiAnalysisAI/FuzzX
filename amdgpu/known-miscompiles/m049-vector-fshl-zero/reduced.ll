; RUN-INPUTS: 0x00000100
; RUN-LLVM-BUILD: build/llvm-fuzzer
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %idx64 = zext i32 0 to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %x = load i32, ptr addrspace(1) %in.ptr, align 4
  %vec = insertelement <4 x i32> zeroinitializer, i32 %x, i32 3
  %fshl = call <4 x i32> @llvm.fshl.v4i32(<4 x i32> %vec, <4 x i32> zeroinitializer, <4 x i32> zeroinitializer)
  %result = extractelement <4 x i32> %fshl, i32 3
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %result, ptr addrspace(1) %out.ptr, align 4
  ret void
}

declare <4 x i32> @llvm.fshl.v4i32(<4 x i32>, <4 x i32>, <4 x i32>)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
