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
  %lz = call i32 @llvm.ctlz.i32(i32 %x, i1 false)
  %lo = trunc i32 %lz to i8
  %xlo = xor i8 %lo, 45
  %wide = zext i8 %xlo to i32
  %mixed = xor i32 %lz, %wide
  %mixed_lo = trunc i32 %mixed to i8
  %mixed_lo_wide = zext i8 %mixed_lo to i32
  %result = xor i32 %mixed, %mixed_lo_wide
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %result, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.ctlz.i32(i32, i1 immarg)

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
