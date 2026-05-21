; Reproduces NaN-asymmetry miscompile in performFNegCombine FMED3 arm
; (AMDGPUISelLowering.cpp:5383-5401):
;
;   fneg(fmed3 x, y, z)  ->  fmed3(-x, -y, -z)
;
; The fold negates all three operands.  Under IEEE 754 semantics with
; nnan, fneg distributes through min/max/med; but v_med3_f32 treats NaN
; ASYMMETRICALLY -- NaN sorts as smaller-than-everything regardless of
; the NaN's sign bit.  So negating the three operands does NOT yield a
; sign-flipped result when one of them is NaN: the NaN keeps its
; "smaller-than-everything" position rather than swapping to
; "larger-than-everything".
;
; Concrete: med3(NaN, 1.0, 2.0) = 1.0, so fneg = -1.0.
;          med3(-NaN, -1.0, -2.0) = -2.0 (NaN still sorts smallest,
;          median of {-NaN, -1, -2} after dropping NaN is -2).
;
; Test value: x = +qNaN (0x7FC00000), y = 1.0, z = 2.0.
;   Expected (O0 / canonical):  -1.0 = 0xBF800000
;   Observed (O2 / fold fires): -2.0 = 0xC0000000
;
; Asm divergence (gfx950):
;   O0: v_med3_f32 v1, v1, v2, v3       (then v_sub for fneg)
;   O2: v_med3_f32 v1, -v1, -v2, -v3    (fneg folded into NEG modifier)
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m110-performfnegcombine-fmed3-nan-asymmetry/reduced.ll

source_filename = "m110-performfnegcombine-fmed3-nan-asymmetry"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.amdgcn.fmed3.f32(float, float, float)
declare noundef i32 @llvm.amdgcn.workitem.id.x() #1
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %workgroup = call i32 @llvm.amdgcn.workgroup.id.x()
  %workitem  = call i32 @llvm.amdgcn.workitem.id.x()
  %base      = mul i32 %workgroup, 256
  %idx       = add i32 %base, %workitem
  %in.range  = icmp eq i32 %idx, 0
  br i1 %in.range, label %body, label %exit

body:
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xi = load volatile i32, ptr addrspace(1) %p0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %yi = load volatile i32, ptr addrspace(1) %p1
  %p2 = getelementptr i32, ptr addrspace(1) %in, i64 2
  %zi = load volatile i32, ptr addrspace(1) %p2
  %x = bitcast i32 %xi to float
  %y = bitcast i32 %yi to float
  %z = bitcast i32 %zi to float

  %m = call float @llvm.amdgcn.fmed3.f32(float %x, float %y, float %z)
  %r = fsub float -0.0, %m       ; canonical fneg(m)

  %rbits = bitcast float %r to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %rbits, ptr addrspace(1) %o0
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x7fc00000, 0x3f800000, 0x40000000
; (x = +qNaN, y = 1.0, z = 2.0;
;  expected r = -1.0 = 0xBF800000;
;  observed r = -2.0 = 0xC0000000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
