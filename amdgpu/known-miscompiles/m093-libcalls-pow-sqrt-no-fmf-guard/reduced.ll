; RUN-INPUTS: [0xff800000, 0x80000000]
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

; AMDGPULibCalls::fold_pow rewrites pow(x, 0.5) -> sqrt(x) unconditionally
; (file AMDGPULibCalls.cpp, lines 936-950 -- no fast-math/finite-only/nsz
; gating).  This is wrong for two well-known IEEE/C99 corner cases:
;
;   x = -Inf : pow(-Inf, 0.5) per C99 == +Inf, but sqrt(-Inf)        == NaN.
;   x = -0.0 : pow(-0.0, 0.5) per C99 == +0.0, but sqrt(-0.0)        == -0.0.
;
; This reproducer exercises both inputs.  At O0 the libcall pass is not run
; (TargetMachine::registerPeepholeEPCallback bails at OptimizationLevel::O0),
; so the in-module definition of _Z3powff below executes and produces the
; correct C99 answers.  At O2 the call site is rewritten to _Z4sqrtf, which
; we provide here as a thin wrapper around llvm.sqrt.f32, yielding the
; IEEE-sqrt answers and visibly diverging from O0.

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %wi to i64
  %ip = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %xi = load i32, ptr addrspace(1) %ip, align 4
  %x = bitcast i32 %xi to float
  %r = tail call float @_Z3powff(float %x, float 5.000000e-01)
  %ri = bitcast float %r to i32

  %op = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %ri, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

; In-module OCL pow.  Returns the C99/IEEE answer for the two corner inputs
; this reproducer exercises (x = -Inf and x = -0.0 with y = 0.5).  All other
; inputs punt to llvm.pow.f32.
define float @_Z3powff(float %x, float %y) #1 {
entry:
  %is_half = fcmp oeq float %y, 5.000000e-01
  %is_neg_inf = fcmp oeq float %x, 0xFFF0000000000000
  %is_zero = fcmp oeq float %x, 0.000000e+00
  %x_bits = bitcast float %x to i32
  %sign_set = icmp slt i32 %x_bits, 0
  %is_neg_zero = and i1 %is_zero, %sign_set
  %match_inf = and i1 %is_half, %is_neg_inf
  %match_zero = and i1 %is_half, %is_neg_zero
  br i1 %match_inf, label %ret_pos_inf, label %check_zero

check_zero:
  br i1 %match_zero, label %ret_pos_zero, label %fallback

ret_pos_inf:
  ret float 0x7FF0000000000000

ret_pos_zero:
  ret float 0.000000e+00

fallback:
  %p = call float @llvm.pow.f32(float %x, float %y)
  ret float %p
}

; A definition is required for AMDGPULibFunc::getFunction to return non-null
; (it skips declaration-only callees), so this body must exist for the
; pow->sqrt fold to even fire at O2.
define float @_Z4sqrtf(float %x) #1 {
  %s = call float @llvm.sqrt.f32(float %x)
  ret float %s
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare float @llvm.pow.f32(float, float)
declare float @llvm.sqrt.f32(float)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nounwind "target-cpu"="gfx950" }
