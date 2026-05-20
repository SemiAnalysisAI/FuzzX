; RUN-INPUTS: 0x0
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  ; 2^127 = 0x47E0000000000000 (as f64-encoded ConstantFP for f32 = 0x7F000000).
  ; Exact 1.0 / 2^127 = 2^-127 = 0x00400000 (a denormal f32).
  ; gfx950's default f32 flush-to-zero mode flushes v_rcp_f32 denormal
  ; results, but the IR constant folder doesn't.
  %r = call float @llvm.amdgcn.rcp.f32(float 0x47E0000000000000)
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
