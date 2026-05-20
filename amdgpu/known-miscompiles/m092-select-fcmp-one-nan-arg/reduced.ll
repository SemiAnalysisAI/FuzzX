; RUN-INPUTS: 0x7fc00000,0x40000000
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

; SITargetLowering::performSelectCombine in SIISelLowering.cpp rewrites:
;   select (fcmp one x, K), other, K  ->  select (fcmp one x, K), other, x
; The transform requires K to be a "normal" non-inline-immediate FP constant.
; It does NOT require %x to be non-NaN.
;
; When %x is NaN, the original semantics are:
;   fcmp one NaN, K  => false (unordered)
;   select (false), other, K  =>  K
; The folded semantics are:
;   select (false), other, NaN  =>  NaN
;
; So the optimised code returns NaN where O0 returns the constant K.

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  ; Load %x (will be a NaN bit pattern) and %other.
  %x = load volatile float, ptr addrspace(1) %in, align 4
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %other = load volatile float, ptr addrspace(1) %p1, align 4

  ; K must be a normal, non-inline-immediate constant.  0x4005BF0A00000000
  ; matches the test in select-cmp-shared-constant-fp.ll (truncated to
  ; ~2.71875).
  %cmp = fcmp one float %x, 0x4005BF0A00000000
  %sel = select i1 %cmp, float %other, float 0x4005BF0A00000000

  %idx64 = zext i32 %wi to i64
  %op = getelementptr float, ptr addrspace(1) %out, i64 %idx64
  store volatile float %sel, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
