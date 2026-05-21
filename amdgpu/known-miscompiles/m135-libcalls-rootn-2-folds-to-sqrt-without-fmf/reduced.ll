; RUN-INPUTS: [0x80000000, 0xff800000]
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

; AMDGPULibCalls::fold_rootn rewrites rootn(x, 2) -> llvm.sqrt(x)
; (AMDGPULibCalls.cpp lines 1171-1189) without any FMF/nsz/ninf gating.
; Per OpenCL 3.0 builtins spec, rootn(x, n) returns x^(1/n) with these
; corner cases for even n > 0:
;
;   rootn(-0.0, 2) = +0.0  (sign of zero NOT preserved; (-0)^(0.5) = +0)
;   rootn(-Inf, 2) = NaN   (negative base with even root, undefined)
;
; Per IEEE 754, llvm.sqrt has the standard sqrt corners:
;
;   sqrt(-0.0) = -0.0  (sign preserved)
;   sqrt(-Inf) = NaN
;
; So for x = -0.0, expected r = +0.0 (0x00000000), observed r = -0.0
; (0x80000000) -- a sign-of-zero divergence.
;
; This is the rootn(2) analog of m093 (pow(x, 0.5) -> sqrt) but the
; rootn fold uses CreateUnaryIntrinsic(Intrinsic::sqrt) directly so it
; does not even need a module-visible _Z4sqrtf definition to fire.

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %wi to i64
  %ip = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %xi = load i32, ptr addrspace(1) %ip, align 4
  %x = bitcast i32 %xi to float
  %r = tail call float @_Z5rootnfi(float %x, i32 2)
  %ri = bitcast float %r to i32

  %op = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %ri, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

; In-module OCL rootn covering the corner cases this reproducer exercises:
;   rootn(-0.0, 2) -> +0.0
;   rootn(-Inf, 2) -> NaN
; For other inputs we just punt to llvm.sqrt (since rootn(x, 2) = sqrt(x)
; for x >= 0).  This is enough to expose the divergence vs the fold.
define float @_Z5rootnfi(float %x, i32 %n) #1 {
entry:
  %is_two = icmp eq i32 %n, 2
  %is_zero = fcmp oeq float %x, 0.000000e+00
  %xbits = bitcast float %x to i32
  %is_neg = icmp slt i32 %xbits, 0
  %is_neg_zero = and i1 %is_zero, %is_neg
  %match_neg_zero = and i1 %is_two, %is_neg_zero
  br i1 %match_neg_zero, label %ret_pos_zero, label %check_neg_inf

check_neg_inf:
  %is_neg_inf = fcmp oeq float %x, 0xFFF0000000000000
  %match_neg_inf = and i1 %is_two, %is_neg_inf
  br i1 %match_neg_inf, label %ret_qnan, label %fallback

ret_pos_zero:
  ret float 0.000000e+00

ret_qnan:
  ret float 0x7FF8000000000000

fallback:
  %s = call float @llvm.sqrt.f32(float %x)
  ret float %s
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare float @llvm.sqrt.f32(float)

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nounwind "target-cpu"="gfx950" }
