; SDAG twin of m075/m077: AMDGPUTargetLowering::performRcpCombine
; (AMDGPUISelLowering.cpp:5549-5558) folds AMDGPUISD::RCP(C) to
; APFloat(1.0)/Val without consulting the kernel's denormal mode.
; The in-source comment is "XXX - Should this flush denormals?".
;
; Trigger from valid IR (no @llvm.amdgcn.rcp): `fdiv afn 1.0, C` is
; rewritten by lowerFastUnsafeFDIV (SIISelLowering.cpp:13117) into
; AMDGPUISD::RCP, then performRcpCombine constant-folds the RCP.
; The result is a denormal that hardware would FTZ but the fold keeps
; full-precision.
;
; With C = 0x47E0000000000000 (= 2.0**127), true 1/C = 2.0**-127 is
; subnormal in f32 (= 0x00400000).  Hardware v_rcp_f32(2.0**127) under
; `denormal-fp-math-f32=preserve-sign,preserve-sign` produces +0.0
; (= 0x00000000).  The SDAG constant-fold path keeps 0x00400000.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m104-sdag-rcp-constant-denormal/reduced.ll

source_filename = "m104-sdag-rcp-constant-denormal"
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
  ; Volatile load is just to keep the kernel non-trivial.  The buggy
  ; constant fold is on the divisor side.
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %x  = load volatile i32, ptr addrspace(1) %p0

  ; 1.0 / 2.0**127  (= 2**-127, subnormal in f32).
  %r  = fdiv afn float 1.0, 0x47E0000000000000

  %rbits = bitcast float %r to i32
  %use   = xor i32 %rbits, %x

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %use, ptr addrspace(1) %o0
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "denormal-fp-math-f32"="preserve-sign,preserve-sign" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x00000000

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
