; RUN-INPUTS: 0
; RUN-COMBINED: 1
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release

source_filename = "m029-fshl-select-phi.ll"
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %block.base = mul i32 %wg, 256
  %idx = add i32 %block.base, %wi
  %ok = icmp ult i32 %idx, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %idx to i64
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %fshl = call i32 @llvm.fshl.i32(i32 %n, i32 %n, i32 14)
  %mask = add i32 %wi, 32
  %masked = and i32 %fshl, %mask
  %x = xor i32 %masked, -1
  %is0 = icmp eq i32 %wi, 0
  br i1 %is0, label %then, label %else

then:
  %one = add i32 %wi, 1
  br label %join

else:
  br label %join

join:
  %p = phi i32 [ %one, %then ], [ 0, %else ]
  %sum = add i32 %p, %wi
  %y = xor i32 %sum, %x
  %cmp = icmp sgt i32 %y, %x
  %and = and i32 %y, %x
  %result = select i1 %cmp, i32 0, i32 %and
  store i32 %result, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workgroup.id.x()
declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.fshl.i32(i32, i32, i32)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
