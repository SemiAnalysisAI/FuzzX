; RUN-INPUTS: 0*2
; RUN-COMBINED: 1
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release

source_filename = "m030-ctlz-shl-or-bitop3.ll"
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
  %shl.n = shl i32 %n, 31
  %ctlz = call i32 @llvm.ctlz.i32(i32 %shl.n, i1 false)
  %nz = icmp ne i32 %ctlz, 0
  %z = zext i1 %nz to i32
  %base = shl i32 2147467263, 14
  %add = add i32 %base, %z
  %result = or i32 %add, %z
  store i32 %result, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.ctlz.i32(i32, i1 immarg)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
