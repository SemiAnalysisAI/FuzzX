; RUN-INPUTS: 0x00000000
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

; fmul.legacy(-2.0, +0.0) must yield +0.0 by the V_MUL_LEGACY_F32 spec
; (0 * anything == +0, sign of zero ignored). InstCombine's
; canSimplifyLegacyMulToMul accepts when one operand is a finite-nonzero
; constant (here -2.0) and rewrites to a regular fmul. But regular IEEE
; fmul preserves the sign of zero: -2.0 * +0.0 = -0.0. So the rewrite
; flips the sign of the result when the runtime operand is +0.0.

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  ; Load the runtime float (input is 0x00000000 == +0.0).
  %x_i = load i32, ptr addrspace(1) %in, align 4
  %x = bitcast i32 %x_i to float
  ; Legacy mul: result must be +0.0 regardless of the other operand's sign.
  %r = call float @llvm.amdgcn.fmul.legacy(float -2.0, float %x)
  %ri = bitcast float %r to i32

  %idx64 = zext i32 %wi to i64
  %op = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %ri, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare float @llvm.amdgcn.fmul.legacy(float, float)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
