; RUN-INPUTS: 0x0*256
; RUN-COMBINED: 1
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release

source_filename = "known-miscompiles/m035-wave-reduce-xor-constant/reduced.ll"
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out) #0 {
entry:
  %idx = call i32 @llvm.amdgcn.workitem.id.x()
  %idx64 = zext i32 %idx to i64
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %reduced = call i32 @llvm.amdgcn.wave.reduce.xor.i32(i32 30, i32 0)
  store i32 %reduced, ptr addrspace(1) %out.ptr, align 4
  ret void
}

declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1
declare i32 @llvm.amdgcn.wave.reduce.xor.i32(i32, i32 immarg) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { convergent nocallback nofree nounwind willreturn memory(none) }
