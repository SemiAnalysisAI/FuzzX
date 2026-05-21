; Reproduces fcanonicalize(<2 x half> undef) decaying to <0.0, 0.0>
; instead of <qNaN, qNaN> in performFCanonicalizeCombine
; (SIISelLowering.cpp:15885-15924).
;
; LangRef `llvm.canonicalize` on undef should produce a quiet NaN
; (the canonical value).  The scalar undef arm at line 15868 handles
; `N->getOperand(0).isUndef()` correctly, returning qNaN.
;
; But SDAG lowers `<2 x half> undef` as `BUILD_VECTOR undef, undef`,
; not as a single undef SDValue.  The combine then takes the v2f16
; build-vector path at 15885+ instead:
;   - Lane 1 fixup at 15917-15921 sees both NewElts undef and sets
;     NewElts[1] = 0.0 (correct fallback per its branch).
;   - Lane 0 fixup at 15910-15915 then sees NewElts[1]=CFP(0.0) and
;     splats to NewElts[0] = NewElts[1] = 0.0.
;
; Result: `<0.0, 0.0>` (= 0x00000000 packed) instead of
;         `<qNaN, qNaN>` (= 0x7E007E00 packed).
;
; This is distinct from m115 (m115 = lane-0 undef + lane-1 runtime-
; canonicalize, leaks raw register bits).  m124 = both lanes undef,
; decays to a non-NaN finite value.
;
; Sibling type v4f16 correctly produces qNaN per lane because type-
; legalization splits to scalar undef arms.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m124-fcanonicalize-v2f16-both-undef-decays-to-zero/reduced.ll

source_filename = "m124-fcanonicalize-v2f16-both-undef-decays-to-zero"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare <2 x half> @llvm.canonicalize.v2f16(<2 x half>)

define amdgpu_kernel void @t(ptr addrspace(1) %out) {
  ; fcanonicalize of <2 x half> undef should produce <qNaN, qNaN>.
  %c = call <2 x half> @llvm.canonicalize.v2f16(<2 x half> undef)
  %bc = bitcast <2 x half> %c to i32
  store i32 %bc, ptr addrspace(1) %out
  ret void
}

; Compare with v4f16: type-legalizes to two scalar fcanon(undef) calls,
; each of which hits the correct scalar undef arm and returns qNaN.
declare <4 x half> @llvm.canonicalize.v4f16(<4 x half>)

define amdgpu_kernel void @t_v4(ptr addrspace(1) %out) {
  %c = call <4 x half> @llvm.canonicalize.v4f16(<4 x half> undef)
  %bc = bitcast <4 x half> %c to i64
  store i64 %bc, ptr addrspace(1) %out
  ret void
}

; Expected: store 0x7E007E00 for t (qNaN, qNaN packed)
;           store 0x7E007E007E007E00 for t_v4
; Observed: store 0x00000000 for t (BUG)
;           store 0x7E007E007E007E00 for t_v4 (correct, via scalar path)
