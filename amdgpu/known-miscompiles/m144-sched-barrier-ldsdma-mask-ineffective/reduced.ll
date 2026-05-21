; m144: `llvm.amdgcn.sched.barrier` with mask = 0x800 (allow LDSDMA
; past barrier) is silently ineffective on gfx950.
;
; Documented semantics (AMDGPUUsage.rst:1626): "All LDSDMA
; instructions may be scheduled across sched_barrier" when bit 0x800
; (= 2048) is set in the mask.
;
; AMDGPUIGroupLP.cpp:2667-2676 (`invertSchedBarrierMask`) clears the
; aggregate `VMEM` bit when LDSDMA-allow is requested, but leaves the
; VMEM_READ (0x10) and VMEM_WRITE (0x20) sub-bits set.  Because
; `SIInstrInfo::isLDSDMA` (SIInstrInfo.h:631) is defined as
; `(isVALU && (isMUBUF || isFLAT)) || TENSOR_CNT` -- and every
; LDSDMA instruction therefore also satisfies `isVMEM && mayLoad/Store`
; -- `canAddMI` (AMDGPUIGroupLP.cpp:2474-2480) classifies the LDSDMA
; instruction into the SchedGroup via the VMEM_READ / VMEM_WRITE
; branches.  The instruction receives ordering edges to the
; SCHED_BARRIER and cannot move past, contradicting the documented
; "may be scheduled across" semantics.
;
; Asymmetry: requesting DS=allow (mask = 0x80) correctly allows LDSDMA
; (line 2680 clears the LDSDMA bit when DS is allowed), but the
; reverse path -- requesting LDSDMA itself -- fails to allow LDSDMA.
;
; This reproducer uses `amdgcn.global.load.lds` (an LDSDMA op) with
; an `amdgcn.sched.barrier(2048)` between two dependent uses of the
; LDS destination.  The barrier should allow the LDSDMA to schedule
; past it; in practice the scheduler keeps it pinned.

source_filename = "m144-sched-barrier-ldsdma-mask-ineffective"
target triple = "amdgcn-amd-amdhsa"

declare void @llvm.amdgcn.global.load.lds(ptr addrspace(1), ptr addrspace(3), i32, i32, i32)
declare void @llvm.amdgcn.sched.barrier(i32)
declare void @llvm.amdgcn.s.waitcnt(i32)

@lds = internal addrspace(3) global [256 x i32] zeroinitializer, align 16

define amdgpu_kernel void @t(ptr addrspace(1) %src, ptr addrspace(1) %dst) {
  ; LDSDMA: load from global into LDS slot 0.
  call void @llvm.amdgcn.global.load.lds(
      ptr addrspace(1) %src,
      ptr addrspace(3) getelementptr ([256 x i32], ptr addrspace(3) @lds, i64 0, i64 0),
      i32 4, i32 0, i32 0)

  ; Documented: this barrier with mask 0x800 should allow LDSDMA to
  ; move past it.  In practice the inverter leaves VMEM_READ/WRITE
  ; bits set, so the scheduler keeps the LDSDMA pinned.
  call void @llvm.amdgcn.sched.barrier(i32 2048)

  ; Independent VALU work after the barrier.
  %x = load i32, ptr addrspace(1) %src, align 4
  %y = add i32 %x, 1
  store i32 %y, ptr addrspace(1) %dst, align 4
  ret void
}
