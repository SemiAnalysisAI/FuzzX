; Reproduces miscompile of i64 sdiv (INT32_MIN, -1):
; AMDGPUTargetLowering::LowerSDIVREM (AMDGPUISelLowering.cpp:2415-2430)
; narrows i64 SDIVREM to i32 whenever both operands have ComputeNumSignBits > 32.
; That admits LHS = sext(INT32_MIN) (33 sign bits). The narrowed i32
; sdiv(0x80000000, -1) is poison; lowering wraps to 0x80000000, and the
; outer SIGN_EXTEND produces -2^31 (0xFFFFFFFF_80000000). The well-defined
; i64 result is +2^31 (0x00000000_80000000).
;
; This bug is mirrored in AMDGPUCodeGenPrepare::expandDivRem32
; (AMDGPUCodeGenPrepare.cpp:1219), so O0 and O2 agree wrong unless
; InstCombine pre-folds the divisor. To force a clean mismatch we use
; a literal `-1` divisor: O2 InstCombine folds `sdiv x, -1` into `0 - x`
; (correct), while O0 takes the buggy narrowing.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m103-lowersdivrem-i64-int32min-narrowing/reduced.ll

source_filename = "m103-lowersdivrem-i64-int32min-narrowing"
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
  ; LHS = sext(loaded_i32) -- ComputeNumSignBits >= 33 for INT32_MIN input.
  %p0  = getelementptr i32, ptr addrspace(1) %in, i64 0
  %lo  = load volatile i32, ptr addrspace(1) %p0
  %lo64 = sext i32 %lo to i64

  ; RHS = sext(loaded_i32) -- volatile so InstCombine can't see the -1.
  %p1  = getelementptr i32, ptr addrspace(1) %in, i64 1
  %rh  = load volatile i32, ptr addrspace(1) %p1
  %rh64 = sext i32 %rh to i64

  ; sdiv i64 will be lowered via the narrow path. Truncating away the
  ; high half exposes the poison.
  %q   = sdiv i64 %lo64, %rh64
  %qhi = lshr i64 %q, 32
  %qhi32 = trunc i64 %qhi to i32
  %qlo32 = trunc i64 %q to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %qlo32, ptr addrspace(1) %o0
  %o1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 %qhi32, ptr addrspace(1) %o1
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x80000000, 0xffffffff
; (lo64 = sext(INT32_MIN) = -2147483648, rh64 = sext(-1) = -1.
;  True quotient = +2147483648 = 0x00000000_80000000; observed = -2^31.)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
