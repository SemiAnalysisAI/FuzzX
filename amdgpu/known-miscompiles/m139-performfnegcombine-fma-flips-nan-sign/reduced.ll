; fp-aux seed=20 mode=v2f16
source_filename = "fp_aux_seed_20_v2f16"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare noundef i32 @llvm.amdgcn.workitem.id.x() #1
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1
declare <2 x half> @llvm.fma.v2f16(<2 x half>, <2 x half>, <2 x half>) #1
declare <2 x half> @llvm.canonicalize.v2f16(<2 x half>) #1
declare <2 x half> @llvm.minnum.v2f16(<2 x half>, <2 x half>) #1
declare <2 x half> @llvm.maxnum.v2f16(<2 x half>, <2 x half>) #1

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %b = mul i32 %wg, 256
  %idx = add i32 %b, %wi
  %ok = icmp eq i32 %idx, 0
  br i1 %ok, label %body, label %exit

body:
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xi0 = load volatile i32, ptr addrspace(1) %p0
  %in0 = bitcast i32 %xi0 to <2 x half>
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %xi1 = load volatile i32, ptr addrspace(1) %p1
  %in1 = bitcast i32 %xi1 to <2 x half>
  %p2 = getelementptr i32, ptr addrspace(1) %in, i64 2
  %xi2 = load volatile i32, ptr addrspace(1) %p2
  %in2 = bitcast i32 %xi2 to <2 x half>
  %v0 = call contract nsz <2 x half> @llvm.fma.v2f16(<2 x half> %in0, <2 x half> %in2, <2 x half> %in0)
  %v1 = call <2 x half> @llvm.minnum.v2f16(<2 x half> %v0, <2 x half> %v0)
  %v2 = fsub contract nsz <2 x half> <half -0.0, half -0.0>, %in1
  %v3 = fadd <2 x half> %v0, %in0
  %v4 = call nsz <2 x half> @llvm.maxnum.v2f16(<2 x half> %in1, <2 x half> %v2)
  %v5 = call <2 x half> @llvm.maxnum.v2f16(<2 x half> %v3, <2 x half> %in1)
  %v6 = fsub contract <2 x half> <half -0.0, half -0.0>, %v0
  %v7 = bitcast <2 x half> %v6 to i32
  %op = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %v7, ptr addrspace(1) %op
  br label %exit

exit:
  ret void
}

; RUN-INPUTS: 0xfe00fc00, 0x3c00bc00, 0x7c007c00

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
