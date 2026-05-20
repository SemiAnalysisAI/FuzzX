; RUN-INPUTS: 0xAABBCCDD
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in,
                                       ptr addrspace(1) %out,
                                       i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64  = zext i32 %wi to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in,  i64 %idx64
  %x      = load i32, ptr addrspace(1) %in.ptr, align 4
  %x.uni  = call i32 @llvm.amdgcn.readfirstlane(i32 %x)
  ; fshl(x, 0, 8) must equal x << 8.
  %r       = call i32 @llvm.fshl.i32(i32 %x.uni, i32 0, i32 8)
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %r, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.amdgcn.readfirstlane(i32)
declare i32 @llvm.fshl.i32(i32, i32, i32)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
