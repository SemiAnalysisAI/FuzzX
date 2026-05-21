; RUN-INPUTS: 0x00007fc0
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"
declare float @llvm.canonicalize.f32(float)
declare double @llvm.canonicalize.f64(double)
declare half @llvm.canonicalize.f16(half)
declare bfloat @llvm.canonicalize.bf16(bfloat)
declare <2 x half> @llvm.canonicalize.v2f16(<2 x half>)
declare <2 x bfloat> @llvm.canonicalize.v2bf16(<2 x bfloat>)
declare float @llvm.fabs.f32(float)
declare half @llvm.fabs.f16(half)
declare bfloat @llvm.fabs.bf16(bfloat)
declare float @llvm.copysign.f32(float, float)
declare half @llvm.copysign.f16(half, half)
declare bfloat @llvm.copysign.bf16(bfloat, bfloat)
declare float @llvm.maxnum.f32(float, float)
declare float @llvm.minnum.f32(float, float)
declare float @llvm.maximum.f32(float, float)
declare float @llvm.minimum.f32(float, float)
declare half @llvm.maxnum.f16(half, half)
declare half @llvm.minnum.f16(half, half)
declare bfloat @llvm.maxnum.bf16(bfloat, bfloat)
declare bfloat @llvm.minnum.bf16(bfloat, bfloat)
declare float @llvm.amdgcn.fract.f32(float)
declare float @llvm.amdgcn.rcp.f32(float)
declare float @llvm.amdgcn.rsq.f32(float)
declare float @llvm.amdgcn.sqrt.f32(float)
declare float @llvm.amdgcn.log.f32(float)
declare float @llvm.amdgcn.exp2.f32(float)
declare float @llvm.amdgcn.frexp.mant.f32(float)
declare noundef i32 @llvm.amdgcn.workitem.id.x() #1
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1
declare half @llvm.fma.f16(half, half, half)

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %workgroup = call i32 @llvm.amdgcn.workgroup.id.x()
  %workitem  = call i32 @llvm.amdgcn.workitem.id.x()
  %base = mul i32 %workgroup, 256
  %idx  = add i32 %base, %workitem
  %ok   = icmp eq i32 %idx, 0
  br i1 %ok, label %body, label %exit
body:
  %ip0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xi0 = load volatile i32, ptr addrspace(1) %ip0
  %xh = trunc i32 %xi0 to i16
  %xf = bitcast i16 %xh to bfloat
  %v0 = insertelement <2 x bfloat> poison, bfloat %xf, i32 1
  %c = call <2 x bfloat> @llvm.canonicalize.v2bf16(<2 x bfloat> %v0)
  %ri0 = bitcast <2 x bfloat> %c to i32
  %op0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %ri0, ptr addrspace(1) %op0
  br label %exit
exit:
  ret void
}
attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "denormal-fp-math"="preserve-sign,preserve-sign" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
