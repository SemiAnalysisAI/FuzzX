; m149: SIPreAllocateWWMRegs.cpp:102 skips AV-class virtregs on
; gfx90A/gfx950 -> WWM inactive-lane corruption.
;
; processDef at SIPreAllocateWWMRegs.cpp:102 early-exits when
; `TRI->isVGPR(*MRI, Reg)` is false.  `isVGPR` calls `isVGPRClass`,
; which is true only when a regclass has VGPRs and NO AGPRs.
;
; On gfx90A+ (gfx950 included) MAI-capable classes are unified
; vector super-classes (AV_32 / AV_64 / etc.; see
; SIRegisterInfo.cpp:3645-3651 getCompatibleSubRegClass and
; isVectorSuperClass at SIRegisterInfo.h:253-255).  AV classes
; satisfy `hasVGPRs && hasAGPRs`, so `isVGPRClass(RC) == false` and
; the WWM pre-allocator silently drops the virtreg.
;
; Effects:
; 1. The virtreg is left to the per-thread VGPRAllocator, which is
;    unaware of WWM semantics.
; 2. The physreg is never added to `WWMReservedRegs`, so the regular
;    VGPRAllocator may reuse it across EXIT_STRICT_WWM for another
;    live virtreg.
; 3. Post-EXIT writes execute under restored EXEC and overwrite only
;    active lanes, corrupting inactive-lane data the WWM live range
;    was supposed to preserve.
;
; The same defect exists in SILowerWWMCopies.cpp:135 addToWWMSpills.
;
; Reproducer wraps an MFMA result (defines AV_*) in
; llvm.amdgcn.strict.wwm.  Subsequent VALU work uses the same VGPR
; range with restored EXEC.

source_filename = "m149-preallocate-wwm-skips-av-class-gfx950"
target triple = "amdgcn-amd-amdhsa"

declare <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(float, float, <16 x float>, i32, i32, i32)
declare <16 x float> @llvm.amdgcn.strict.wwm.v16f32(<16 x float>)

define amdgpu_kernel void @t(ptr addrspace(1) %p) {
  %z   = load <16 x float>, ptr addrspace(1) %p
  %mf  = call <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(
      float 1.0, float 1.0, <16 x float> %z, i32 0, i32 0, i32 0)
  ; Wrap the MFMA result (AV_512) in strict.wwm.  WWM semantics
  ; require all lanes of %wwm to be preserved across the strict
  ; region, including inactive lanes.
  %wwm = call <16 x float> @llvm.amdgcn.strict.wwm.v16f32(<16 x float> %mf)
  ; Independent VALU work after EXIT_STRICT_WWM that may reuse the
  ; same VGPR range and overwrite inactive lanes of %wwm.
  %sum = fadd <16 x float> %wwm, %z
  store <16 x float> %sum, ptr addrspace(1) %p
  ret void
}
