; RUN-INPUTS: 0, 1
; RUN-LLVM-BUILD: build/llvm-fuzzer
source_filename = "m014-shl-add-ctpop"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %base = mul i32 %wg, 256
  %idx = add i32 %base, %wi
  %ok = icmp ult i32 %idx, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %idx to i64
  %inptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %x = load i32, ptr addrspace(1) %inptr, align 4
  %t0 = shl i32 %x, 1
  %t1 = add i32 %t0, 1
  %t2 = shl i32 %t1, 1
  %t3 = add i32 %t2, 1
  %t4 = shl i32 %t3, 1
  %t5 = add i32 %t4, 1
  %t6 = shl i32 %t5, 1
  %t7 = add i32 %t6, 1
  %result = call i32 @llvm.ctpop.i32(i32 %t7)
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %result, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workgroup.id.x()
declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.ctpop.i32(i32)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
