; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %tid = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %tid, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %tid to i64
  %a = xor i32 %tid, 16
  %masked = and i32 %a, 18
  %v = insertelement <1 x i32> zeroinitializer, i32 %masked, i32 0
  %vand = and <1 x i32> %v, <i32 16>
  %e = extractelement <1 x i32> %vand, i32 0
  %r = xor i32 %masked, %e
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %r, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
