; RUN-INPUTS: 0x40000000,0x40400000
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %a_i = load i32, ptr addrspace(1) %in, align 4
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %b_i = load i32, ptr addrspace(1) %p1, align 4
  %a = bitcast i32 %a_i to float
  %b = bitcast i32 %b_i to float
  ; fmed3(2.0, 3.0, qNaN) with ieee=0 -> min(2.0, 3.0) = 2.0  (per AMD ISA)
  %r = call nnan ninf float @llvm.amdgcn.fmed3.f32(
                          float %a, float %b,
                          float 0x7FF8000000000000)
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

; "amdgpu-ieee"="false" forces the IEEE-off code path in the InstCombine
; transform of amdgcn.fmed3.
attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" "amdgpu-ieee"="false" }
