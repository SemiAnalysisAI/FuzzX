; RUN-INPUTS: 0*13,0x000000f0
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %idx64 = zext i32 %wi to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %num = add i32 %wi, 13762291
  %den.mask = and i32 %v, 255
  %den = or i32 %den.mask, 1
  %q = sdiv i32 %num, %den
  store i32 %q, ptr addrspace(1) %out.ptr, align 4
  ret void
}

declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
