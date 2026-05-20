; RUN-INPUTS: 0x0
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

; m077 candidate: amdgcn.rcp.f32 constant folder ignores f32 flush of the
; DENORMAL INPUT (sibling to m075, which handles the denormal-OUTPUT case).
;
; Constant input bits 0x00400000 decode as f32 denormal value 2^-127.
; HW v_rcp_f32 on gfx950 with the default PreserveSign f32 mode flushes
; the denormal input to +0, then v_rcp_f32(+0) = +Inf = 0x7f800000.
;
; The IR fold (AMDGPUInstCombineIntrinsic.cpp:1097-1106) just computes
; 1.0 / 2^-127 = 2^127 with APFloat and replaces the call with the
; constant 0x7f000000 -- a finite normal value the hardware would never
; emit for this input.
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %r = call float @llvm.amdgcn.rcp.f32(float bitcast (i32 4194304 to float))
  %ri = bitcast float %r to i32
  %idx64 = zext i32 %wi to i64
  %op = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %ri, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare float @llvm.amdgcn.rcp.f32(float)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
