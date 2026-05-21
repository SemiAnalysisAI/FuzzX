; RUN-INPUTS: 0x12345678,0xCAFEBABE,0xDEADBEEF,0x55AA33CC,0xFFFFFFFF,0x00000000,0xA5A5A5A5,0x5A5A5A5A
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %wi64 = zext i32 %wi to i64
  %offa = add i64 %wi64, 0
  %pa = getelementptr i32, ptr addrspace(1) %in, i64 %offa
  %a = load i32, ptr addrspace(1) %pa, align 4
  %offb = add i64 %wi64, 1
  %pb = getelementptr i32, ptr addrspace(1) %in, i64 %offb
  %b = load i32, ptr addrspace(1) %pb, align 4
  %offc = add i64 %wi64, 2
  %pc = getelementptr i32, ptr addrspace(1) %in, i64 %offc
  %c = load i32, ptr addrspace(1) %pc, align 4

  %v0 = or i32 %b, %a
  %v1 = xor i32 %v0, -1
  %v2 = and i32 %c, %v1
  %v3 = and i32 %v2, %v1
  %v4 = xor i32 %v2, %c
  %v7 = and i32 %v3, %v4

  %idx64 = zext i32 %wi to i64
  %op_p = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %v7, ptr addrspace(1) %op_p, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
