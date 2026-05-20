; RUN-INPUTS: 0x0
; RUN-LLVM-BUILD: build/llvm-fuzzer
; NOTE: requires a wave32 GPU (e.g. gfx1030); on a wave64-only system this
; is a static (assembly-level) miscompile, demonstrated by comparing -O0
; and -O2 asm. There is no wave32 hardware in the FuzzX CI box so this
; reproducer cannot be auto-run by run_ll_reproducer.sh against gfx950.
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
entry:
  ; fcmp.i64 with two equal constant inputs (OEQ -> always-true) is folded
  ; in AMDGPUInstCombineIntrinsic to read_register("exec", i64).  On wave32
  ; targets EXEC is conceptually 32 bits (EXEC_LO); reading the 64-bit EXEC
  ; register pair puts EXEC_HI (architecturally unused on wave32) into the
  ; high 32 bits of the result.  The SDAG path used at -O0 instead emits a
  ; v_cmp + zext i32->i64, producing high bits == 0.
  %r = call i64 @llvm.amdgcn.fcmp.i64.f32(float 0.0, float 0.0, i32 1)
  store i64 %r, ptr addrspace(1) %out, align 8
  ret void
}

declare i64 @llvm.amdgcn.fcmp.i64.f32(float, float, i32 immarg)

attributes #0 = { convergent nounwind "target-cpu"="gfx1030" }
