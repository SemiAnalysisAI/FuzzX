; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %tid = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %tid, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %tid to i64
  %inptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %x = load i32, ptr addrspace(1) %inptr, align 4
  %a = xor i32 %x, -1414812756
  %lo = trunc i32 %a to i8
  %shifted = shl i8 %lo, 3
  %wide = zext i8 %shifted to i32
  %mixed = xor i32 %a, %wide
  %v0 = insertelement <2 x i32> zeroinitializer, i32 %mixed, i32 0
  %v1 = insertelement <2 x i32> %v0, i32 126, i32 1
  %vec = shl <2 x i32> %v1, zeroinitializer
  %e = extractelement <2 x i32> %vec, i32 0
  %result = xor i32 %mixed, %e
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %result, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
