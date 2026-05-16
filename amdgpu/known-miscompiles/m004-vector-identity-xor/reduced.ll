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
  %v0 = insertelement <2 x i32> zeroinitializer, i32 %x, i32 0
  %v1 = insertelement <2 x i32> %v0, i32 1, i32 1
  %mul = mul <2 x i32> %v1, <i32 1, i32 -1>
  %e1 = extractelement <2 x i32> %mul, i32 1
  %y1 = xor i32 %x, %e1
  %a0 = insertelement <2 x i32> zeroinitializer, i32 %y1, i32 0
  %a1 = insertelement <2 x i32> %a0, i32 1, i32 1
  %addv = add <2 x i32> %a1, <i32 1, i32 1>
  %e2 = extractelement <2 x i32> %addv, i32 1
  %y2 = xor i32 %y1, %e2
  %w0 = insertelement <2 x i32> zeroinitializer, i32 %y2, i32 0
  %w1 = insertelement <2 x i32> %w0, i32 1, i32 1
  %sub = sub <2 x i32> %w1, zeroinitializer
  %e = extractelement <2 x i32> %sub, i32 0
  %result = xor i32 %y2, %e
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %result, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
