; RUN-INPUTS: 0xFFFFFFFE
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %v = load i32, ptr addrspace(1) %in, align 4
  %x = or i32 %v, 1                                 ; provably non-zero, bits 31..1 unknown
  %sffbh = call i32 @llvm.amdgcn.sffbh.i32(i32 %x)
  %r = call i32 @llvm.umin.i32(i32 %sffbh, i32 32)
  %idx64 = zext i32 %wi to i64
  %op = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %r, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.amdgcn.sffbh.i32(i32)
declare i32 @llvm.umin.i32(i32, i32)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
