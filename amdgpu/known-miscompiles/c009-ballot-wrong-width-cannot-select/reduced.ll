; c009: `llvm.amdgcn.ballot.<N>` with `<N> != WavefrontSize` ICEs at
; -O2 whenever the argument is not a constant 0/1.
;
; SIISelLowering.cpp:7811-7852 (lowerBALLOTIntrinsic) emits
; AMDGPUISD::SETCC in the user-requested return type without first
; emitting the wave-sized SETCC and zext/trunc'ing.  ISel has no
; pattern matching a wave-mask SETCC at the wrong width, so it
; aborts:
;
;   LLVM ERROR: Cannot select: i32 = AMDGPUISD::SETCC ..., setne:ch
;
; Reproduces on both ballot.i32 / wave64 (gfx950) and ballot.i64 /
; wave32 (gfx1030 +wavefrontsize32).
;
; Distinct from c007 (constant-fold ICE on equal constant operands) --
; c009 fires for arbitrary non-constant inputs and is unrelated to
; constant folding.

source_filename = "c009-ballot-wrong-width-cannot-select"
target triple = "amdgcn-amd-amdhsa"

declare i32 @llvm.amdgcn.ballot.i32(i1)

define amdgpu_kernel void @t(ptr addrspace(1) %p, i32 %x) {
  %c = icmp eq i32 %x, 0
  %r = call i32 @llvm.amdgcn.ballot.i32(i1 %c)   ; i32 on wave64 -> ICE
  store i32 %r, ptr addrspace(1) %p, align 4
  ret void
}
