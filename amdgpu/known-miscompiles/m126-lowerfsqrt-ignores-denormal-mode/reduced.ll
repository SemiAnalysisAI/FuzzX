; Reproduces denormal-mode unsafety in lowerFSQRTF32 and lowerFSQRTF64
; (SIISelLowering.cpp:13682-13862).
;
; Both branches of lowerFSQRTF32 emit `v_fma_f32` NR/residual chains
; with NO `AMDGPUISD::DENORM_MODE` toggle.  Compare LowerFDIV32
; (13334-13468) which wraps its NR chain in `S_SETREG_B32 /
; DENORM_MODE` writes (13379-13416 / 13436-13462) to force IEEE
; denormals around the FMAs, saving and restoring under
; `denormal-fp-math-f32="preserve-sign,..."`.
;
; Under f32 FTZ kernels, the `SqrtE = 0.5 - SqrtH*SqrtS` NR term is
; subnormal whenever `SqrtR ~ 2^-127` (i.e., for very large x), and
; gets flushed to +/-0 -- blocking NR convergence.  The pre-scaling
; at line 13700 only bounds the small side (`x < 2^-96`); large
; inputs (e.g., x > 2^126) still produce subnormal NR intermediates.
;
; lowerFSQRTF64 (13772-13862) has the same shape: f64 NR chain
; (rsq_f64 / fmul / fma * 7) with no denorm-mode toggle.  Under
; `denormal-fp-math="preserve-sign,preserve-sign"` the v_fma_f64
; chain runs with FTZ; near-denormal intermediates flush to zero
; and NR converges to wrong values.
;
; Also missing: `Flags.setNoFPExcept(true)` (compare LowerFDIV32
; line 13343).
;
; Sibling of m122 (fdiv64 denormal mode).
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m126-lowerfsqrt-ignores-denormal-mode/reduced.ll

source_filename = "m126-lowerfsqrt-ignores-denormal-mode"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare double @llvm.sqrt.f64(double)
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
  ; x = double bits loaded volatile.
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xlo = load volatile i32, ptr addrspace(1) %p0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %xhi = load volatile i32, ptr addrspace(1) %p1
  %xlo64 = zext i32 %xlo to i64
  %xhi64 = zext i32 %xhi to i64
  %xhi64s = shl i64 %xhi64, 32
  %xbits = or i64 %xhi64s, %xlo64
  %x = bitcast i64 %xbits to double

  %r = call double @llvm.sqrt.f64(double %x)

  %rbits = bitcast double %r to i64
  %rlo = trunc i64 %rbits to i32
  %rhi64 = lshr i64 %rbits, 32
  %rhi = trunc i64 %rhi64 to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %rlo, ptr addrspace(1) %o0
  %o1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 %rhi, ptr addrspace(1) %o1
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "denormal-fp-math"="preserve-sign,preserve-sign" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x00000000, 0x00100000
; (x = 2^-1022 = smallest normal f64;
;  expected r = sqrt(2^-1022) ~ 2^-511 (= 0x2000000000000000);
;  observed may differ under FTZ NR convergence error)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
