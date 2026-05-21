; RUN-INPUTS: 0x12345678,0xCAFEBABE,0xDEADBEEF
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %a    = load i32, ptr addrspace(1) %in, align 4
  %pb   = getelementptr i32, ptr addrspace(1) %in, i64 1
  %b    = load i32, ptr addrspace(1) %pb, align 4
  %pc   = getelementptr i32, ptr addrspace(1) %in, i64 2
  %c    = load i32, ptr addrspace(1) %pc, align 4

  ; Inline v4 into v7: r = v2 ^ ((~v2) | v0)
  %v0 = xor i32 %c, %b
  %v1 = or  i32 %v0, %a
  %v2 = and i32 %v1, %v0
  %not_v2 = xor i32 %v2, -1
  %v7 = or  i32 %not_v2, %v0
  %r  = xor i32 %v2, %v7

  %idx64 = zext i32 %wi to i64
  %op    = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %r, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
