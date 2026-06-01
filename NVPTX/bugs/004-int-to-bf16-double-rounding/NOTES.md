# 004 — i32/i64 -> bf16 conversion double-rounds (int->f32->bf16), producing wrong result for inputs that round inexactly to f32

- **Kind:** miscompile
- **Reachable via:** llc -mcpu=sm_80 (sm<90 || ptx<78)
- **Component:** NVPTXISelLowering.cpp 2461-2475 (LowerINT_TO_FP); action setup 986-994  (region `L07-fp-lowering`)
- **Candidate id:** c009

## Summary

`sitofp/uitofp i32/i64 -> bfloat` double-rounds via f32, giving the wrong correctly-rounded result

## Mechanism / root cause

When the result type is bf16, LowerINT_TO_FP lowers `sitofp/uitofp <int> to bfloat` by first converting the integer to f32 and then FP_ROUNDing f32->bf16:

  if (Op.getValueType() == MVT::bf16) {
    return DAG.getNode(ISD::FP_ROUND, Loc, MVT::bf16,
        DAG.getNode(Op.getOpcode(), Loc, MVT::f32, Op.getOperand(0)),
        DAG.getIntPtrConstant(0, Loc, /*isTarget=*/true));
  }

This path is selected for source types i1/i16/i32/i64 on targets with sm<90 || ptx<78 (setOperationAction loop at lines 987-990). The emitted PTX is `cvt.rn.f32.s32` followed by `cvt.rn.bf16.f32` (verified with llc below).

For i32/i64 the int->f32 step is itself a rounding (values above 2^24 are not exactly representable in f32's 24-bit significand), so the result is rounded twice: once to f32 (24 bits), once to bf16 (8 bits). Double rounding diverges from the correctly-rounded direct int->bf16 result whenever the first rounding lands an inexact value exactly onto a bf16 tie point that then rounds the 'wrong' way under round-to-nearest-even.

Concrete witness: x = 33685505 (an i32 in [2^25, 2^26), where bf16 ULP=2^18 and f32 ULP=4).
 - Correct sitofp i32 33685505 to bfloat: 33685505 is just above the bf16 midpoint 33685504 (=257*2^17) between bf16 values 33554432 (=256*2^18) and 33816576 (=258*2^18), so it rounds UP to 33816576.0.
 - Emitted path: cvt.rn.f32.s32 rounds 33685505 to the nearest multiple of 4 = 33685504.0 (distance 1 < 3). That f32 value is exactly the bf16 midpoint, so cvt.rn.bf16.f32 rounds to even -> 33554432.0.
 Result: PTX yields 33554432.0 but the IR requires 33816576.0. (Confirmed numerically: direct bf16(33685505)=33816576.0, f32(33685505)=33685504.0, bf16 of that = 33554432.0.)

The i1/i16 source cases are safe (they fit exactly in f32, so only one rounding occurs). The reverse direction (LowerFP_TO_INT, bf16->f32->int) is fine because bf16->f32 is exact. A correct lowering would round-inexact-to-odd through the f32 intermediate (as LowerFP_ROUND already does for f64->bf16 via expandRoundInexactToOdd), or use a wider-precision/direct conversion, to avoid the double rounding.

## Trigger

Target sm_80..sm_89 or any config with sm<90 || ptx<78 (e.g. -mcpu=sm_80). Source type i32 or i64 (values exceeding 2^24 in magnitude that round inexactly to f32 and land on a bf16 tie). sitofp/uitofp/SINT_TO_FP/UINT_TO_FP with bf16 result.

## Reproducer

See `repro.ll` / `cmd.sh`.

```
define bfloat @i32_to_bf16(i32 %x) {
  %r = sitofp i32 %x to bfloat
  ret bfloat %r
}

; Independent oracle: LLVM's own constant folder gives the CORRECT 0x4C01.
define bfloat @i32_to_bf16_const() {
  %r = sitofp i32 33685505 to bfloat
  ret bfloat %r
}

define bfloat @i64_to_bf16(i64 %x) {
  %r = sitofp i64 %x to bfloat
  ret bfloat %r
}
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_80 -O2 -o - repro.ll
```

## Observed (wrong) output

```
i32_to_bf16 (runtime path, BUGGY double rounding):
	ld.param.b32 	%r1, [i32_to_bf16_param_0];
	cvt.rn.f32.s32 	%r2, %r1;
	cvt.rn.bf16.f32 	%rs1, %r2;
	st.param.b16 	[func_retval0], %rs1;
For input %x = 33685505: cvt.rn.f32.s32 -> 33685504.0, then cvt.rn.bf16.f32 (exact bf16 tie -> round to even) -> 33554432.0 (bf16 bits 0x4C00). WRONG.

i32_to_bf16_const (constant-fold path, CORRECT oracle):
	st.param.b16 	[func_retval0], 0x4C01;   // = 33816576.0

i64_to_bf16 (same double-rounding bug):
	cvt.rn.f32.s64 	%r1, %rd1;
	cvt.rn.bf16.f32 	%rs1, %r1;
```

## Expected

sitofp i32 33685505 to bfloat must round once, directly to the nearest bf16 value. 33685505 is above the bf16 midpoint 33685504 (between bf16 reps 33554432 and 33816576), so the correctly-rounded result is 33816576.0 (bf16 bits 0x4C01). LLVM's constant folder produces exactly this. A correct backend lowering would avoid double rounding — e.g. round-inexact-to-odd through the f32 intermediate (as LowerFP_ROUND already does for f64->bf16 via expandRoundInexactToOdd) or a direct/wider conversion — so that the runtime path also yields 33816576.0 (0x4C01) instead of the emitted 33554432.0 (0x4C00).

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.82, verify confidence 0.97).

> CONFIRMED MISCOMPILE via double rounding in int->bf16 lowering.

Source mechanism (real, not a misreading): NVPTXISelLowering.cpp LowerINT_TO_FP (lines 2465-2471) lowers `sitofp/uitofp <int> to bfloat` as: int -> f32 (original opcode) then ISD::FP_ROUND f32 -> bf16. This is registered Custom for source types i1/i16/i32/i64 with bf16 result on targets where sm<90 || ptx<78 (setOperationAction loop lines 987-994). Tellingly, the sibling LowerFP_ROUND (line 2512) deliberately uses TLI->expandRoundInexactToOdd for f64->bf16 precisely to avoid double rounding, but LowerINT_TO_FP does NOT — it does a naive double rounding.

Emitted PTX (verified by running the provided llc): for the runtime function `i32_to_bf16`, llc emits `cvt.rn.f32.s32 %r2, %r1` then `cvt.rn.bf16.f32 %rs1, %r2`. Both steps use IEEE round-to-nearest-even (.rn). The i64 case emits the analogous `cvt.rn.f32.s64` + `cvt.rn.bf16.f32`.

Concrete defined, non-UB input: i32 33685505 (well within i32 range; sitofp is defined for all integers).

Numerical discrepancy (verified independently in Python AND against LLVM's own constant folder):
- 33685505 lies in [2^25,2^26); bf16 ULP = 2^18, between bf16 reps 33554432 (=128*2^18, even) and 33816576 (=129*2^18, odd), midpoint = 33685504. Since 33685505 > midpoint, correct direct round-to-nearest-even = 33816576.0 (bits 0x4C01).
- Hardware path: cvt.rn.f32.s32(33685505): f32 ULP=4 in this range, candidates 33685504 (dist 1) and 33685508 (dist 3) -> rounds to 33685504.0 (unamb
