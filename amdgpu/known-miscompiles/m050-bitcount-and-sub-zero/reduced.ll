; RUN-INPUTS: 0x00000000*256
; RUN-LLVM-BUILD: build/llvm-fuzzer
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %base = mul i32 %wg, 256
  %idx = add i32 %base, %wi
  %idx64 = zext i32 %idx to i64
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  br label %loop

loop:
  %iv = phi i32 [ 0, %entry ], [ %next, %body ]
  %acc = phi i32 [ 2147483597, %entry ], [ %acc.next, %body ]
  %cond = icmp ult i32 %iv, 1
  br i1 %cond, label %body, label %exit

body:
  %lo = lshr i32 %acc, 31
  %hi = shl i32 -1, 31
  %or = or i32 %lo, %hi
  %masked = xor i32 %or, -1
  %cmp = icmp ult i32 %masked, 1
  %sel = select i1 %cmp, i32 -1, i32 0
  %set = or i32 %masked, %sel
  %dec = sub i32 %set, 0
  %clear = and i32 %set, %dec
  %pop.a = call i32 @llvm.ctpop.i32(i32 %set)
  %pop.clear = call i32 @llvm.ctpop.i32(i32 %clear)
  %delta = sub i32 %pop.a, %pop.clear
  %mix = add i32 %clear, %delta
  %acc.next = xor i32 %mix, %iv
  %next = add i32 %iv, 1
  br label %loop

exit:
  store i32 %acc, ptr addrspace(1) %out.ptr, align 4
  ret void
}

declare i32 @llvm.amdgcn.workgroup.id.x()
declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.ctpop.i32(i32)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
