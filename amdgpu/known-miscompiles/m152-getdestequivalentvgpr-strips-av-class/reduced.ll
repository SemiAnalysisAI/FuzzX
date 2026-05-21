; m152: SIInstrInfo::getDestEquivalentVGPRClass strips AV-class dest
; on gfx90A/gfx950 -> register-class mismatch / implicit truncation.
;
; Direct sibling of m149 (SIPreAllocateWWMRegs).  Same isVGPRClass
; blindness in the moveToVALU helper.
;
; SIInstrInfo.cpp:9684-9688 SrcRC-not-AGPR branch:
;
;   if (RI.isVGPRClass(NewDstRC) || NewDstRC == &AMDGPU::VReg_1RegClass)
;     return nullptr;
;   NewDstRC = RI.getEquivalentVGPRClass(NewDstRC);
;
; isVGPRClass (SIRegisterInfo.h:243) is `hasVGPRs(RC) && !hasAGPRs(RC)`.
; AV_* classes have AGPR bits -> return false.  On gfx90A+ (gfx950)
; getLargestLegalSuperClass (SIRegisterInfo.cpp:468) promotes
; VReg_/AReg_ to AV_*, so AV-class virtregs are the norm for
; COPY/PHI/REG_SEQUENCE/INSERT_SUBREG dests.  The early bail-out is
; skipped and the dest is replaced by getEquivalentVGPRClass(AV_xx),
; a strict-VGPR class.

source_filename = "m152-getdestequivalentvgpr-strips-av-class"
target triple = "amdgcn-amd-amdhsa"

declare <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(float, float, <16 x float>, i32, i32, i32)

define amdgpu_kernel void @t(ptr addrspace(1) %p) {
  %z = load <16 x float>, ptr addrspace(1) %p
  ; MFMA result is in AV-class virtreg.
  %mf = call <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(
      float 1.0, float 1.0, <16 x float> %z, i32 0, i32 0, i32 0)
  ; A divergent extract that triggers moveToVALU on the AV-class def.
  ; getDestEquivalentVGPRClass strips the AGPR legality from the dest.
  %idx = call i32 @llvm.amdgcn.workitem.id.x()
  %e = extractelement <16 x float> %mf, i32 %idx
  store float %e, ptr addrspace(1) %p
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
