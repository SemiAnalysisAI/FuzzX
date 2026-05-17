; RUN-INPUTS: 0x00000000
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release

source_filename = "m032-loop-vector-select.ll"
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
  %argbool = icmp eq i32 %wi, 999
  br label %loop.header

loop.header:
  %iv = phi i32 [ 0, %body ], [ 1, %else ]
  %acc = phi i32 [ 0, %body ], [ %result, %else ]
  %cond = icmp ult i32 %iv, 1
  br i1 %cond, label %loop.body, label %loop.exit

loop.body:
  %v0 = insertelement <2 x i32> zeroinitializer, i32 %wi, i32 0
  %v1 = insertelement <2 x i32> %v0, i32 1431655765, i32 1
  %v2 = insertelement <2 x i32> zeroinitializer, i32 %wi, i32 1
  %mul = mul <2 x i32> %v1, %v2
  %e0 = extractelement <2 x i32> %mul, i32 0
  %e1 = extractelement <2 x i32> %mul, i32 1
  %sub = sub i32 %e0, %e1
  %branch = icmp sle i32 %sub, -1923363058
  br i1 %branch, label %early, label %else

early:
  ret void

else:
  %w0 = insertelement <4 x i32> zeroinitializer, i32 %sub, i32 1
  %cmp = icmp sle <4 x i32> %w0, zeroinitializer
  %selv = select <4 x i1> %cmp, <4 x i32> zeroinitializer, <4 x i32> <i32 3, i32 0, i32 0, i32 0>
  %x = extractelement <4 x i32> %selv, i32 0
  %is_small = icmp slt i32 %x, 1
  %z = zext i1 %is_small to i32
  %lz = call i32 @llvm.ctlz.i32(i32 %z, i1 false)
  %result = select i1 %argbool, i32 %lz, i32 1
  br label %loop.header

loop.exit:
  store i32 %acc, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.ctlz.i32(i32, i1 immarg)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
