; RUN-INPUTS: 0
; RUN-COMBINED: 1
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release

source_filename = "m038-loop-fp-mask-xor.ll"
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %wi to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %mask.input = and i32 %v, 1023
  %mask.block = and i32 %wg, 1023
  %fp.input = uitofp i32 %mask.input to float
  %fp.block = uitofp i32 %mask.block to float
  %fp.mul = fmul float %fp.input, %fp.block
  %fp.zero = fptoui float %fp.mul to i32
  %all.ones = or i32 %v, -1
  %outer.trip.mask = and i32 %all.ones, 3
  %outer.trip = add i32 %outer.trip.mask, 1
  br label %outer.header

outer.header:
  %outer.iv = phi i32 [ 0, %body ], [ %outer.next, %inner.exit ]
  %outer.acc = phi i32 [ %all.ones, %body ], [ %outer.acc.next, %inner.exit ]
  %outer.cond = icmp ult i32 %outer.iv, %outer.trip
  br i1 %outer.cond, label %inner.header, label %outer.exit

inner.header:
  %inner.iv = phi i32 [ 0, %outer.header ], [ %inner.next, %inner.continue ]
  %inner.acc = phi i32 [ %outer.acc, %outer.header ], [ %inner.acc.next, %inner.continue ]
  %inner.cond = icmp ult i32 %inner.iv, 4
  br i1 %inner.cond, label %inner.body, label %inner.exit

inner.body:
  %mask = and i32 %inner.acc, 1023
  %as.float = uitofp i32 %mask to float
  %input.float = uitofp i32 %mask.input to float
  %sum = fadd float %as.float, %input.float
  %as.int = fptoui float %sum to i32
  %xor = xor i32 %as.int, %inner.acc
  %break = icmp sge i32 %xor, %mask.input
  br i1 %break, label %inner.exit, label %inner.continue

inner.continue:
  %inner.acc.next = xor i32 %xor, %inner.iv
  %inner.next = add i32 %inner.iv, 1
  br label %inner.header

inner.exit:
  %inner.result = phi i32 [ %inner.acc, %inner.header ], [ %xor, %inner.body ]
  %outer.acc.next = xor i32 %inner.result, %outer.iv
  %outer.next = add i32 %outer.iv, 1
  br label %outer.header

outer.exit:
  %result.mask = and i32 %outer.acc, 1023
  %zero.mask = and i32 %fp.zero, 1023
  %result.float = uitofp i32 %result.mask to float
  %zero.float = uitofp i32 %zero.mask to float
  %result.double = fpext float %result.float to double
  %zero.double = fpext float %zero.float to double
  %sum.double = fadd double %result.double, %zero.double
  %sum.float = fptrunc double %sum.double to float
  %sum.int = fptoui float %sum.float to i32
  %result = add i32 %sum.int, %fp.zero
  store i32 %result, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.amdgcn.workgroup.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
