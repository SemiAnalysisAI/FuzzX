# c009 — i32/i64 -> bf16 conversion double-rounds (int->f32->bf16), producing wrong result for inputs that round inexactly to f32

- region: L07-fp-lowering
- file: NVPTXISelLowering.cpp 2461-2475 (LowerINT_TO_FP); action setup 986-994
- kind: miscompile
- confidence(finder): 0.82

## Mechanism
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

## IR
```
define bfloat @i32_to_bf16(i32 %x) {
  %r = sitofp i32 %x to bfloat
  ret bfloat %r
}
; call with %x = 33685505 -> should be 33816576.0 but PTX computes 33554432.0
```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_80 -O2`
