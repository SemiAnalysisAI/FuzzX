; Reproduces sNaN escape through canonicalize via amdgcn.frexp.mant
; (and ~12 sibling intrinsics):
; SITargetLowering::isCanonicalized (SIISelLowering.cpp:15672) lists
; Intrinsic::amdgcn_frexp_mant as unconditionally canonical, with no
; check that the input is itself canonical.
;
; HW v_frexp_mant_f32 propagates NaN payload unchanged (LLVM intrinsic
; semantics + AMD ISA: NaN in = NaN out, including sNaN preserved).
; So canonicalize(frexp_mant(sNaN)) should yield a canonical qNaN but
; the COPY-pattern fcanonicalize_canonicalized (SIInstrInfo.td:1013)
; lowers fcanonicalize-of-known-canonical to COPY, leaking the sNaN.
;
; The codebase contradicts itself: known-never-snan.ll line 566
; (v_test_NOT_known_frexp_mant_input_fmed3_r_i_i_f32) explicitly
; treats frexp_mant's output as possibly sNaN.
;
; Same shape applies to: amdgcn.rcp, amdgcn.rsq, amdgcn.sqrt,
; amdgcn.exp2, amdgcn.log, amdgcn.trig_preop, amdgcn.cubeid,
; amdgcn.cvt_pkrtz, amdgcn.fdot2, amdgcn.rcp_legacy, amdgcn.rsq_legacy,
; amdgcn.rsq_clamp -- all listed at SIISelLowering.cpp:15670-15683.
;
; Run with:
;   llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 -O2 reduced.ll
;
; Expected asm: v_frexp_mant_f32 followed by canonicalize ops
; (v_max_f32 v,v,v at minimum)
; Observed asm: v_frexp_mant_f32 alone, then store -- canonicalize
; is elided.  sNaN input emerges as sNaN at output.

source_filename = "m118-iscanonicalized-frexpmant-rcp-sqrt-overclaim"
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.amdgcn.frexp.mant.f32(float)
declare float @llvm.canonicalize.f32(float)

define amdgpu_kernel void @canon_frexp_mant(ptr addrspace(1) %out, float %x) {
  %r = call float @llvm.amdgcn.frexp.mant.f32(float %x)
  %c = call float @llvm.canonicalize.f32(float %r)
  store float %c, ptr addrspace(1) %out
  ret void
}

; Compare baseline (no frexp_mant): emits a real canonicalize op.
define amdgpu_kernel void @canon_baseline(ptr addrspace(1) %out, float %x) {
  %c = call float @llvm.canonicalize.f32(float %x)
  store float %c, ptr addrspace(1) %out
  ret void
}
