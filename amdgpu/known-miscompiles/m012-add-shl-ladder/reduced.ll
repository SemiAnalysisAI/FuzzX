; RUN-INPUTS: 0, 1
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
  %a0 = add i32 %x, 1
  %s0 = shl i32 %a0, 1
  %a1 = add i32 %s0, 1
  %s1 = shl i32 %a1, 1
  %a2 = add i32 %s1, 1
  %s2 = shl i32 %a2, 1
  %a3 = add i32 %s2, 1
  %s3 = shl i32 %a3, 1
  %a4 = add i32 %s3, 1
  %s4 = shl i32 %a4, 1
  %result = add i32 %s4, 1
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %result, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
