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
  %clz = call i32 @llvm.ctlz.i32(i32 %x, i1 false)
  %lo8 = trunc i32 %clz to i8
  %dec = add i8 %lo8, -1
  %wide8 = zext i8 %dec to i32
  %v = xor i32 %clz, %wide8
  %lo16 = trunc i32 %v to i16
  %id = ashr i16 %lo16, 0
  %wide16 = sext i16 %id to i32
  %result = xor i32 %v, %wide16
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %result, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.ctlz.i32(i32, i1 immarg)

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
