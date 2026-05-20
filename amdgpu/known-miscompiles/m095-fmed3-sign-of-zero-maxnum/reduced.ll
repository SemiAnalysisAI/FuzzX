; RUN-INPUTS: 0x00000000
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

; amdgcn.fmed3(-0.0, -0.0, +0.0): hardware v_med3_f32 returns the median of
; the three operands; sorted by sign-of-zero this is {-0, -0, +0}, so the
; median is -0.0 (0x80000000).
;
; The InstCombine constant-fold computes:
;   Max3 = maxnum(maxnum(-0,-0), +0) = +0
;   Max3.compare(Src0=-0) == cmpEqual, so return maxnum(Src1=-0, Src2=+0)
;   = +0  (APFloat maxnum treats +0 > -0)
; producing +0.0 (0x00000000).  -> sign-of-zero miscompile.

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  ; Force a load so the input pointer is "used" (the runner expects this layout).
  %a_i = load volatile i32, ptr addrspace(1) %in, align 4
  ; Pure-constant fmed3: exercises the constant-fold path in instCombineIntrinsic.
  %r = call float @llvm.amdgcn.fmed3.f32(float -0.0, float -0.0, float 0.0)
  %ri = bitcast float %r to i32
  %idx64 = zext i32 %wi to i64
  %op = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %ri, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare float @llvm.amdgcn.fmed3.f32(float, float, float)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
