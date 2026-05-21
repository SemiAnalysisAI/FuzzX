# w58: x86_avx512_mask_{add,sub,mul,div}_{ss,sd}_round with imm=4 (CUR_DIRECTION) -> fadd/fsub/fmul/fdiv drops MXCSR rounding

File: llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp, lines 2487-2549.

## Reasoning

When the rounding-mode operand equals 4 (`_MM_FROUND_CUR_DIRECTION`), the InstCombine fold extracts the lowest element, performs a plain IR `fadd/fsub/fmul/fdiv`, applies the masking, and re-inserts. Plain IR FP ops are *unconstrained*: LLVM may constant-fold them assuming round-to-nearest-even and is not required to honor MXCSR. But the source program said "use current MXCSR", which the user typically sets via `_MM_SET_ROUNDING_MODE` or `fesetround` immediately before the call. Sibling bug w04 covered the packed `_ps_512`/`_pd_512` variants; this is the *scalar masked* variants (`mask_add_ss_round`, `mask_sub_ss_round`, `mask_mul_ss_round`, `mask_div_ss_round`, and `_sd` versions) at a distinct switch arm.

## Concrete IR (verified)

```llvm
target triple = "x86_64-unknown-unknown"
declare <4 x float> @llvm.x86.avx512.mask.add.ss.round(<4 x float>, <4 x float>, <4 x float>, i8, i32 immarg)
declare void @llvm.x86.sse.ldmxcsr(ptr)

define <4 x float> @t(<4 x float> %a, <4 x float> %b, ptr %p) {
  call void @llvm.x86.sse.ldmxcsr(ptr %p)
  %r = call <4 x float> @llvm.x86.avx512.mask.add.ss.round(<4 x float> %a, <4 x float> %b, <4 x float> zeroinitializer, i8 -1, i32 4)
  ret <4 x float> %r
}
```

After `opt -passes=instcombine -S` (with `x86_64-unknown-unknown` triple):

```llvm
define <4 x float> @t(<4 x float> %a, <4 x float> %b, ptr %p) {
  call void @llvm.x86.sse.ldmxcsr(ptr %p)
  %1 = extractelement <4 x float> %a, i64 0
  %2 = extractelement <4 x float> %b, i64 0
  %3 = fadd float %1, %2
  %r = insertelement <4 x float> %a, float %3, i64 0
  ret <4 x float> %r
}
```

The user-installed MXCSR rounding (e.g., round-toward-zero) is silently dropped: any later constant fold of `%3 = fadd float %1, %2` will use round-to-nearest-even and disagree with the hardware semantics of the original intrinsic.

## Expected wrong result

Same demonstration as w04: with `%a[0] = 1.0`, `%b[0] = 0x3e70000000000000` (≈ 5.6e-79) and MXCSR set to round-toward-zero, the hardware `vaddss {rd-sae},...` (rounding=CUR_DIRECTION = read MXCSR) returns exactly `1.0`. After the rewrite, a downstream pass that constant-folds `fadd 1.0, ulp` will use round-to-nearest-even and produce `1.0 + ulp(1.0)`. This is a behavioral divergence between the original intrinsic and the rewritten IR whenever the surrounding code has set MXCSR to a non-default rounding mode.

The fix is symmetric to the proposed fix for w04: only fold when the function has no FP-environment access and the target's default rounding is RNE (i.e., add `strictfp` / FENV awareness gating).
