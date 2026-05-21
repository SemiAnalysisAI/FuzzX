; m141: SITargetLowering::isCanonicalized recurses through ISD::BITCAST
; without consulting source/dest FP semantics.  In-source TODO at
; SIISelLowering.cpp:15649-15653 acknowledges the bug.
;
; A bit pattern that is normal in v2bf16 (8-bit exponent) can be
; denormal in v2f16 (5-bit exponent), and vice versa.  The combiner
; treats the bitcast as canonical-preserving and may drop an explicit
; fcanonicalize that the FTZ-on-next-use HW behaviour requires.
;
; This reproducer exposes the bitcast-canonicality leak by:
;   1. Loading a v2f16 lane whose bit pattern is "normal in v2bf16
;      semantics but denormal in v2f16 semantics".
;   2. Round-tripping bf16 -> i16 -> bf16 -> v2bf16 -> i32 -> v2f16.
;   3. Applying fcanonicalize to the result.
;   4. Observing that O2 may drop the canonicalize, leaving the
;      v2f16 denormal raw; O0 retains the canonicalize and emits
;      a v_max_f16 v, v (or v_pk_max_f16) which FTZs the denormal
;      to +0.

target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @t(ptr addrspace(1) %in, ptr addrspace(1) %out) {
  %xi = load i32, ptr addrspace(1) %in, align 4
  ; The bit pattern in %xi is chosen so that:
  ;   - viewed as v2bf16: both lanes are normal (e.g. 0x3F80_3F80 = 1.0, 1.0)
  ;   - viewed as v2f16: both lanes are subnormal in the corresponding bits
  %v_bf = bitcast i32 %xi to <2 x bfloat>
  ; Pass through the lane permutation; intentionally identity to keep
  ; the bit pattern intact while preventing trivial constant-folding.
  %lo = extractelement <2 x bfloat> %v_bf, i32 0
  %hi = extractelement <2 x bfloat> %v_bf, i32 1
  %v_bf2 = insertelement <2 x bfloat> poison, bfloat %lo, i32 0
  %v_bf3 = insertelement <2 x bfloat> %v_bf2, bfloat %hi, i32 1
  ; Round-trip back through i32 and into v2f16.  This is the
  ; "value-changing bitcast" -- same bits, different denormal class.
  %xi2 = bitcast <2 x bfloat> %v_bf3 to i32
  %v_h = bitcast i32 %xi2 to <2 x half>
  ; Now apply fcanonicalize.  The combiner walks back through the
  ; chain of bitcasts via isCanonicalized and (incorrectly) concludes
  ; that the value is already canonical (because the v2bf16 layer
  ; would canonicalize to itself), so it may drop the canonicalize.
  ; HW v_pk_max_f16 v,v then FTZs the v2f16 subnormals; without the
  ; canonicalize, the raw subnormals reach the store.
  %canon = call <2 x half> @llvm.canonicalize.v2f16(<2 x half> %v_h)
  store <2 x half> %canon, ptr addrspace(1) %out, align 4
  ret void
}

declare <2 x half> @llvm.canonicalize.v2f16(<2 x half>)
