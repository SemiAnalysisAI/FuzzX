; RUN-INPUTS: 0,1
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
  %x0 = load i32, ptr addrspace(1) %inptr, align 4
  %s1 = shl i32 %x0, 1
  %x1 = add i32 %s1, 84017408
  %s2 = shl i32 %x1, 1
  %x2 = add i32 %s2, 84017408
  %s3 = shl i32 %x2, 1
  %x3 = add i32 %s3, 84017408
  %s4 = shl i32 %x3, 1
  %x4 = add i32 %s4, 84017408
  %s5 = shl i32 %x4, 1
  %x5 = add i32 %s5, 84017408
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %x5, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
