; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
entry:
  ; fcmp.i32 with two equal constant inputs (and any predicate) constant-folds
  ; at -O2; on wave64 the folded i32 result clashes with the i64 exec register
  ; and ICEs with "invalid type for register 'exec'".
  %r = call i32 @llvm.amdgcn.fcmp.i32.f32(float 0.0, float 0.0, i32 1)
  store i32 %r, ptr addrspace(1) %out, align 4
  ret void
}

declare i32 @llvm.amdgcn.fcmp.i32.f32(float, float, i32 immarg)

attributes #0 = { convergent nounwind "target-cpu"="gfx950" }
