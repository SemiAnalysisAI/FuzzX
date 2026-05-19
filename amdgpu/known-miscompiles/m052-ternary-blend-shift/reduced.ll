; RUN-INPUTS: 0xffffffff
; RUN-LLVM-BUILD: build/llvm-fuzzer
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %is.lane4 = icmp eq i32 %wi, 4
  br i1 %is.lane4, label %body, label %exit

body:
  %v = load i32, ptr addrspace(1) %in, align 4
  %mix = xor i32 %v, 2027808484
  br label %loop

loop:
  %iv = phi i32 [ 0, %body ], [ %next, %loop.body ]
  %acc = phi i32 [ %mix, %body ], [ %fshr, %loop.body ]
  %cond = icmp ult i32 %iv, 3
  br i1 %cond, label %loop.body, label %store

loop.body:
  %mask = xor i32 %acc, %wi
  %not = xor i32 %mask, -1
  %right = and i32 %wi, %not
  %blend = or i32 %mask, %right
  %shift = and i32 %blend, 31
  %inv.raw = sub i32 32, %shift
  %inv = and i32 %inv.raw, 31
  %zero = icmp eq i32 %shift, 0
  %fshr.left = shl i32 %blend, %inv
  %fshr.right = lshr i32 %wi, %shift
  %fshr.raw = or i32 %fshr.left, %fshr.right
  %fshr = select i1 %zero, i32 %wi, i32 %fshr.raw
  %next = add i32 %iv, 1
  br label %loop

store:
  store i32 %acc, ptr addrspace(1) %out, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
