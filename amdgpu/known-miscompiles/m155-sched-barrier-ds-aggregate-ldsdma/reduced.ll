; m155: amdgcn.sched.barrier(0x800) (LDSDMA-allow) still blocks
; LDSDMA via the DS aggregate bit -- m144 sibling.
;
; `invertSchedBarrierMask` (AMDGPUIGroupLP.cpp:2667-2685) clears
; VMEM/VMEM_READ/VMEM_WRITE when LDSDMA is allowed (lines 2668-2676),
; but the symmetric DS clause does NOT check the LDSDMA bit.
; Result: InvertedMask for input 2048 = 2031 (0b011111101111) still
; has DS (0x80) set.
;
; `canAddMI` DS branch (AMDGPUIGroupLP.cpp:2482-2484) matches LDSDMA
; via `isLDSDMA(MI)` on the aggregate DS bit, so the SchedGroup adds
; the LDSDMA op and pins it -- contradicting the
; AMDGPUUsage.rst:1626 contract.
;
; m144 covered the VMEM aggregate; m155 covers the DS aggregate.
; Both must be fixed for amdgcn.sched.barrier(0x800) to actually
; allow LDSDMA past the barrier.

source_filename = "m155-sched-barrier-ds-aggregate-ldsdma"
target triple = "amdgcn-amd-amdhsa"

declare void @llvm.amdgcn.global.load.lds(ptr addrspace(1), ptr addrspace(3), i32, i32, i32)
declare void @llvm.amdgcn.sched.barrier(i32)

@lds = internal addrspace(3) global [256 x i32] zeroinitializer, align 16

define amdgpu_kernel void @t(ptr addrspace(1) %src, ptr addrspace(1) %dst) {
  ; LDSDMA: load from global into LDS slot 0.
  call void @llvm.amdgcn.global.load.lds(
      ptr addrspace(1) %src,
      ptr addrspace(3) getelementptr ([256 x i32], ptr addrspace(3) @lds, i64 0, i64 0),
      i32 4, i32 0, i32 0)

  ; Mask 0x800 (LDSDMA-allow): should let the LDSDMA above schedule
  ; past this barrier.  m155 root cause: DS aggregate stays set in
  ; the inverted mask, and canAddMI classifies LDSDMA via DS branch,
  ; pinning it.
  call void @llvm.amdgcn.sched.barrier(i32 2048)

  ; Independent work after the barrier.
  %x = load i32, ptr addrspace(1) %src, align 4
  %y = add i32 %x, 1
  store i32 %y, ptr addrspace(1) %dst, align 4
  ret void
}
