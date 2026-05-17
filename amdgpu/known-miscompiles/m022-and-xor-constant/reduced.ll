; RUN-INPUTS: 0x0 0x0
; RUN-COMBINED: 1
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release

target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %wi to i64
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %salt = add i32 %wi, -1640531528
  %mix = xor i32 %wi, %salt
  %x = and i32 %mix, %salt
  %xor = xor i32 %x, 2041403025
  %result = and i32 %xor, %x
  store i32 %result, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
