; RUN-INPUTS: 0x00bf2758
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out) #0 {
entry:
  %x = load i32, ptr addrspace(1) %in, align 4
  %in.range = icmp ult i32 %x, 16777216
  call void @llvm.assume(i1 %in.range)
  %den = or i32 %x, 1
  %r = urem i32 %x, %den
  store i32 %r, ptr addrspace(1) %out, align 4
  ret void
}

declare void @llvm.assume(i1 noundef) #1

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(inaccessiblemem: readwrite) }
