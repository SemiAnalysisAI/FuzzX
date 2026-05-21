# w04: x86_avx512_mask_cmp_ss/sd — Arg2 (predicate imm) not validated, Arg4 (SAE) ignored in InstCombine

File: llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp, lines 2423–2443
Intrinsic signature (from IntrinsicsX86.td): `(v4f32, v4f32, i32 imm pred, i8 mask, i32 imm SAE) -> i8`

## Reasoning

The InstCombine handler for `x86_avx512_mask_cmp_ss` / `x86_avx512_mask_cmp_sd` only invokes
`SimplifyDemandedVectorEltsLow` on Arg0 and Arg1 (the scalar-as-vector FP operands), then returns
`&II`. It does not look at Arg2 (predicate immediate 0..31) or Arg4 (SAE/embedded rounding immediate).
That itself is OK for the simplification it performs, but it shares a switch case with the legacy
`vcomi_ss/sd`, which only takes two FP operands. If the simplification ever grows to "convert to
plain `fcmp` when arg4 is the SAE-disabled value", the predicate semantics differ:

* CMPSS predicate 0..7 are the SSE-compatible ordered/unordered variants; 8..31 are AVX-512 extensions.
* Predicates `_NEQ_OQ` (12), `_NLT_UQ` (21), etc., distinguish ordered/unordered and signaling/quiet.
* SAE bit (Arg4 == 8 = `_MM_FROUND_NO_EXC`) suppresses FP invalid-on-QNaN. Folding to an `fcmp`
  predicate that ignores QNaN signaling can lose the trap.

There is also no check that Arg2 actually fits in 5 bits. The `ImmArg` attribute means the
backend trusts the value, but the InstCombiner replaces operand 0/1 with the simplified value
without re-checking that Arg2 is a `ConstantInt` first; if a malformed constant expression slipped
through this would assert in the backend.

## Concrete IR

```llvm
declare i8 @llvm.x86.avx512.mask.cmp.ss(<4 x float>, <4 x float>, i32 immarg, i8, i32 immarg)

define i8 @cmp_ss_sae_with_qnan(<4 x float> %a, <4 x float> %b, i8 %mask) {
  ; predicate 4 = _CMP_NEQ_UQ  (unordered, non-signaling)
  ; SAE = 8 = no FP exceptions
  %r = call i8 @llvm.x86.avx512.mask.cmp.ss(
        <4 x float> <float 0x7FF8000000000000, float 1.0, float 2.0, float 3.0>,
        <4 x float> %b, i32 4, i8 %mask, i32 8)
  ret i8 %r
}
```

After InstCombine SimplifyDemandedVectorEltsLow lifts the lane-0 QNaN into the operand, the
intrinsic survives. But if a future change ever converts this to `fcmp une <float %a0>, <float %b0>`,
that fcmp does not honor the SAE bit (Arg4 = 8) and produces a value with an unordered predicate
that ignores QNaN class entirely.

## Expected wrong result

This is more of a hazard than a current miscompile — file as "audit current handling and grow
test coverage before adding any fold that lowers `mask_cmp_ss` to `fcmp`." The risk surface is
high because the InstCombine for `comi`/`ucomi` already shares the case body, and any future
"fold to fcmp" added here will mishandle the predicate-imm/SAE arguments.
