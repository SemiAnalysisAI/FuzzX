; Reproduces NaN-sign miscompile in VOP3P MadFmaMix TableGen pattern
; (VOP3PInstructions.td:240-251):
;
;   (fsub x, (fpext_f16 y))  ->  v_fma_mix_f32(y, -1.0, x) op_sel_hi:[1,1,0]
;
; This pattern matches canonical IR `fneg(fpext h)` (which is
; `fsub -0.0, fpext(h)`) at O0 and lowers to
; `v_fma_mix_f32(h, -1.0, -0.0)`.  By the same argument as m107, the
; VOP3 NEG src-modifier on the `-1.0` operand does NOT flip the sign of
; the propagated NaN from `h`: HW `v_fma_mix(NaN, -1, -0)` returns
; +NaN (the NaN's input sign bit is preserved).
;
; The performFNegCombine FP_EXTEND arm (AMDGPUISelLowering.cpp:5402-5427)
; correctly handles this: it folds `fneg(fpext h)` -> `fpext(fneg h)`,
; which then becomes `v_cvt_f32_f16(-h)` -- preserves the (flipped)
; sign through the conversion.
;
; The bug is that at O0, the FMA-mix TableGen pattern RACES the FNeg
; combine and wins (DAGCombine runs only at non-zero opt levels).  At
; O2 the FNeg combine fires first and the FMA-mix pattern doesn't
; match.
;
; Test value: x = +qNaN_half (0x7E00).
;   Expected (O2, IEEE): -qNaN = 0xFFC00000
;   Observed (O0):       +qNaN = 0x7FC00000
;
; Asm divergence (gfx950):
;   O0: v_fma_mix_f32 v1, v1, -1.0, s2 op_sel_hi:[1,1,0]
;   O2: v_cvt_f32_f16_e64 v1, -v1
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m111-vop3p-madfmamix-fsub-fpext-nan-sign/reduced.ll

source_filename = "m111-vop3p-madfmamix-fsub-fpext-nan-sign"
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
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xi = load volatile i32, ptr addrspace(1) %p0
  %xh = trunc i32 %xi to i16
  %x  = bitcast i16 %xh to half

  %ext = fpext half %x to float
  %r   = fsub float -0.0, %ext   ; canonical fneg(fpext x)

  %rbits = bitcast float %r to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %rbits, ptr addrspace(1) %o0
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x00007e00
; (x = +qNaN half (0x7E00); expected r = -qNaN = 0xFFC00000;
;  observed r at O0 = +qNaN = 0x7FC00000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
