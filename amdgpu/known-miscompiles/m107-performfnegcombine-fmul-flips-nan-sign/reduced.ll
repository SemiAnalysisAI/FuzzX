; Reproduces NaN-sign miscompile in performFNegCombine FMUL arm
; (AMDGPUISelLowering.cpp:5298-5318):
;
;   fneg(fmul x, y)  ->  fmul(x, fneg y)
;
; The rewrite has NO `nnan` guard (sibling FADD/FMA arms have an
; `nsz` guard but no `nnan` either).  Under IEEE, `fneg(NaN)` is
; the input NaN with its sign bit flipped, and HW `v_xor 0x80000000`
; implements that exactly.  But on AMDGPU HW, `v_mul_f32(NaN, -y)`
; propagates the *input* NaN's sign bit -- the source-modifier on
; the OTHER operand has no effect on a propagated NaN's sign.
;
; So `fneg(fmul(NaN, y)) = -NaN` (sign flipped) but the folded
; `fmul(NaN, -y) = +NaN` (sign preserved).
;
; Test value: x = 0x7FC00000 (+qNaN), y = 1.0.
;   Expected (IEEE/O0): 0xFFC00000 (-qNaN)
;   Observed (O2):      0x7FC00000 (+qNaN)
;
; Asm-level divergence verified at -O0 vs -O2 on gfx950:
;
;   O0:  v_mul_f32_e64 v1, s2, v1
;        v_sub_f32_e64 v1, 0x80000000, v1     ; honors fsub semantics
;   O2:  v_mul_f32_e64 v1, s2, -v1            ; fneg folded into NEG modifier
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m107-performfnegcombine-fmul-flips-nan-sign/reduced.ll

source_filename = "m107-performfnegcombine-fmul-flips-nan-sign"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

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
  ; Volatile-load NaN and 1.0 so the SDAG can't constant-fold the fmul.
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xi = load volatile i32, ptr addrspace(1) %p0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %yi = load volatile i32, ptr addrspace(1) %p1
  %x  = bitcast i32 %xi to float
  %y  = bitcast i32 %yi to float

  %m = fmul float %x, %y         ; v_mul_f32(NaN, 1.0) = NaN with NaN's sign
  %n = fsub float -0.0, %m       ; canonical IR `fneg(m)`; should flip sign

  %nbits = bitcast float %n to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %nbits, ptr addrspace(1) %o0
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x7fc00000, 0x3f800000
; (x = +qNaN, y = +1.0; expected n = -qNaN = 0xFFC00000; observed n = +qNaN = 0x7FC00000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
