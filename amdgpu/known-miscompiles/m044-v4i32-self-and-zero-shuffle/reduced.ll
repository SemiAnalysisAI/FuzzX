; RUN-INPUTS: 0x00000000
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out) #0 {
entry:
  %x = load i32, ptr addrspace(1) %in, align 4
  %masked = and i32 %x, 1
  %a = or i32 %masked, 1
  %v = insertelement <4 x i32> zeroinitializer, i32 %a, i32 0
  %and = and <4 x i32> %v, %v
  %zero.insert = insertelement <4 x i32> zeroinitializer, i32 0, i32 3
  %zero.shuffle = shufflevector <4 x i32> %zero.insert, <4 x i32> %v, <4 x i32> <i32 3, i32 0, i32 0, i32 0>
  %or = or <4 x i32> %and, %zero.shuffle
  %r = extractelement <4 x i32> %or, i32 0
  store i32 %r, ptr addrspace(1) %out, align 4
  ret void
}

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size" }
