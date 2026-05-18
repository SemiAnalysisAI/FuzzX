; RUN-INPUTS: 0x0
; RUN-COMBINED: 1
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release

source_filename = "known-miscompiles/m037-dot4-square-lowbit/reduced.ll"
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %is0 = icmp eq i32 %wi, 0
  br i1 %is0, label %body, label %exit

body:
  %v = load i32, ptr addrspace(1) %in, align 4
  %masked = and i32 %v, 255
  %square = mul i32 %masked, %masked
  %lowbit = and i32 %square, 1
  %result = add i32 %lowbit, %square
  store i32 %result, ptr addrspace(1) %out, align 4
  br label %exit

exit:
  ret void
}

declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
