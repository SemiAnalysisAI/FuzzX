; RUN-INPUTS: 0xf2f2f2fc
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %base = mul i32 %wg, 256
  %idx32 = add i32 %base, %wi
  %active = icmp ult i32 %idx32, %n
  br i1 %active, label %body, label %exit

body:
  %idx = zext i32 %idx32 to i64
  %inptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx
  %loaded = load i32, ptr addrspace(1) %inptr, align 4
  %scalar = call i32 @llvm.amdgcn.readfirstlane(i32 %loaded)
  %result = call i32 @llvm.fshl.i32(i32 %scalar, i32 -218959118, i32 1)
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx
  store i32 %result, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare noundef i32 @llvm.amdgcn.workgroup.id.x()
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.amdgcn.readfirstlane(i32)
declare i32 @llvm.fshl.i32(i32, i32, i32)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
