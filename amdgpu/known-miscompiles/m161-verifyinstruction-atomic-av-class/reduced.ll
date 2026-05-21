; m161: SIInstrInfo::verifyInstruction atomic vdst/vdata file-match
; check uses isAGPR (excludes AV-class) -> spurious verifier reject
; and silently-accepted AV-vs-VGPR mismatch on gfx90A+/gfx950.
;
; SIInstrInfo.cpp:5857-5869 enforces that vdst/vdata of FLAT/MUBUF/DS
; atomics are both AGPR or both VGPR on gfx90A+ via:
;
;   if (RI.isAGPR(MRI, X) != RI.isAGPR(MRI, Y)) { reject }
;
; isAGPR returns false for AV-class (which has BOTH VGPR and AGPR
; bits).  Combined with pre-RA getLargestLegalSuperClass
; (SIRegisterInfo.cpp:468) intentionally widening VReg/AReg pairs to
; AV on gfx90A+, AV-class virtuals are reachable here on gfx950.
;
; Two failure modes:
;   1. AV-class vdst paired with AGPR vdata:
;      isAGPR(vdst)=false != isAGPR(vdata)=true -> verifier
;      spuriously rejects valid IR.
;   2. AV-class vdst paired with VGPR vdata:
;      isAGPR(vdst)=false == isAGPR(vdata)=false -> verifier passes
;      silently, but final allocation may split vdst across A-half
;      and the atomic encoding requires same-file vdst/vdata.
;
; Sibling family: m149 (SIPreAllocateWWMRegs skips AV), m152
; (getDestEquivalentVGPRClass strips AV), m153 (WholeWaveFunction
; prologue EXEC).  Same isVGPRClass/isAGPR-blindness root.

source_filename = "m161-verifyinstruction-atomic-av-class"
target triple = "amdgcn-amd-amdhsa"

; Reproducer: atomic load-modify-write where the IR-level value comes
; from an MFMA result (likely AV-class), paired with a write to a
; flat atomic that would otherwise pair with a true VGPR/AGPR pool.

declare <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(float, float, <16 x float>, i32, i32, i32)

define amdgpu_kernel void @t(ptr addrspace(1) %p) {
  %z   = load <16 x float>, ptr addrspace(1) %p
  %mf  = call <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(
      float 1.0, float 1.0, <16 x float> %z, i32 0, i32 0, i32 0)
  ; Use the AV-class MFMA result as atomic vdata in a buffer atomic.
  %lane0 = extractelement <16 x float> %mf, i32 0
  %lane0i = bitcast float %lane0 to i32
  %r = atomicrmw add ptr addrspace(1) %p, i32 %lane0i seq_cst
  store i32 %r, ptr addrspace(1) %p
  ret void
}
