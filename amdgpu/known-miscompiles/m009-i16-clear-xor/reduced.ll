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
  %b = call i32 @llvm.ctlz.i32(i32 %x, i1 false)
  %c = trunc i32 %b to i16
  %d = add i16 %c, 1
  %e = zext i16 %d to i32
  %f = xor i32 %b, %e
  %g = trunc i32 %f to i16
  %h = add i16 %g, 1
  %i = zext i16 %h to i32
  %j = xor i32 %f, %i
  %k = trunc i32 %j to i16
  %l = add i16 %k, 0
  %m = zext i16 %l to i32
  %clear = xor i32 %j, %m
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %clear, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.ctlz.i32(i32, i1 immarg)

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
