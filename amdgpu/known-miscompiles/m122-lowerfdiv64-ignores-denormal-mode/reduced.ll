; Reproduces denormal-mode unsafety in SITargetLowering::LowerFDIV64
; (SIISelLowering.cpp:13471-13538).
;
; LowerFDIV32 (same file, 13334-13468) wraps its NR chain in
; S_SETREG_B32 / DENORM_MODE to force IEEE denormals around the four
; v_fma_f32 refinement steps, saving/restoring under
; `denormal-fp-math-f32="preserve-sign,..."`.  LowerFDIV64 does none
; of that.
;
; Under `denormal-fp-math="preserve-sign,preserve-sign"` (a legal
; kernel attr on gfx950), the v_fma_f64 refinement runs with f64 FTZ,
; so any near-denormal intermediate (especially in the
; div_scale/fma_chain/div_fmas sequence around very-small or very-large
; divisors) is silently flushed and the NR converges to the wrong
; value.
;
; Both O0 and O2 emit the SAME lowering (same buggy asm with no
; `s_setreg DENORM_MODE`), so the FuzzX O0-vs-O2 oracle does NOT
; catch this.  The witness is SDAG vs IR semantics, or a host
; interpreter.
;
; Sibling of m075/m077/m104 (denormal-mode-blind FP arithmetic) but
; at SDAG f64 div lowering rather than IR-level RCP fold.
;
; Test value: x = 1.0, y = 0x0010000000000000 (smallest f64 normal,
; 2^-1022).  True 1.0/y = 2^1022 (which is normal in f64).  With
; preserve-sign denormal mode and the buggy NR chain, intermediate
; results in the refinement can be denormal and get flushed,
; producing a wrong result.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m122-lowerfdiv64-ignores-denormal-mode/reduced.ll

source_filename = "m122-lowerfdiv64-ignores-denormal-mode"
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
  ; x = 1.0 (load as i64 bits then bitcast to double).
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xlo = load volatile i32, ptr addrspace(1) %p0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %xhi = load volatile i32, ptr addrspace(1) %p1
  %p2 = getelementptr i32, ptr addrspace(1) %in, i64 2
  %ylo = load volatile i32, ptr addrspace(1) %p2
  %p3 = getelementptr i32, ptr addrspace(1) %in, i64 3
  %yhi = load volatile i32, ptr addrspace(1) %p3

  %xlo64 = zext i32 %xlo to i64
  %xhi64 = zext i32 %xhi to i64
  %xhi64s = shl i64 %xhi64, 32
  %xbits = or i64 %xhi64s, %xlo64
  %x = bitcast i64 %xbits to double

  %ylo64 = zext i32 %ylo to i64
  %yhi64 = zext i32 %yhi to i64
  %yhi64s = shl i64 %yhi64, 32
  %ybits = or i64 %yhi64s, %ylo64
  %y = bitcast i64 %ybits to double

  %r = fdiv double %x, %y

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

; RUN-INPUTS: 0x00000000, 0x3ff00000, 0x00000000, 0x00100000
; (x = 1.0 = 0x3FF0000000000000; y = 2^-1022 = 0x0010000000000000 smallest normal;
;  expected r = 2^1022 = 0x7FE0000000000000; observed may differ under FTZ)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
