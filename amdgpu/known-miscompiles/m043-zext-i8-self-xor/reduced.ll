; RUN-INPUTS: 0x00000000
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out) {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %tr = trunc i32 %wi to i8
  %z = zext i8 %tr to i32
  %x = xor i32 %z, 1
  %r = xor i32 %x, %x
  %idx64 = zext i32 %wi to i64
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %r, ptr addrspace(1) %out.ptr, align 4
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
