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
  %pop = call i32 @llvm.ctpop.i32(i32 %x)
  %lo1 = trunc i32 %pop to i8
  %xorlo = xor i8 %lo1, 72
  %wide1 = zext i8 %xorlo to i32
  %v1 = xor i32 %pop, %wide1
  %lo2 = trunc i32 %v1 to i8
  %sublo = sub i8 %lo2, 72
  %wide2 = zext i8 %sublo to i32
  %v2 = xor i32 %v1, %wide2
  %tmp = add i32 %v2, 0
  %lo3 = trunc i32 %tmp to i8
  %id = add i8 %lo3, 0
  %wide3 = zext i8 %id to i32
  %result = xor i32 %tmp, %wide3
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %result, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.ctpop.i32(i32)

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
