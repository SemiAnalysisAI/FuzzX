# 002 — combineMulWide: sext(shl nsw x, topbit) -> mul.wide.s with negative constant miscompiles

- **Kind:** miscompile
- **Reachable via:** default llc -O1+
- **Component:** NVPTXISelLowering.cpp 6379-6385  (region `L01-combines-mul-shl`)
- **Candidate id:** c003

## Summary

`sext(shl nsw x, bitwidth-1)` folded to `mul.wide.s` with a negative constant, negating the result

## Mechanism / root cause

combineMulWide rewrites (sign_extend (shl nsw x, ShiftAmt)) into MUL_WIDE_SIGNED(x, 1<<ShiftAmt). For the SHL case it builds the multiplier constant in the *narrow* type FromVT:

  if (Op.getOpcode() == ISD::SHL) {
    const auto ShiftAmt = Op.getConstantOperandVal(1);
    const auto MulVal = APInt(FromVT.getSizeInBits(), 1) << ShiftAmt;  // narrow width!
    RHS = DCI.DAG.getConstant(MulVal, DL, FromVT);
  }
  return DCI.DAG.getNode(Opcode, DL, ToVT, Op.getOperand(0), RHS);

When ShiftAmt == FromVT.bits-1 (15 for i16, 31 for i32), MulVal sets the high (sign) bit of the narrow constant: for i16, 1<<15 = 0x8000. MUL_WIDE_SIGNED (mul.wide.s16) sign-extends BOTH operands, so the 0x8000 constant is interpreted as the *signed* value -32768, not the intended positive multiplier 2^15 = +32768. The product therefore has the wrong sign.

Concretely, the well-defined (non-poison) input x = -1 (0xFFFF) for `shl nsw i16 %x, 15`: original semantics give shl = 0x8000 = -32768, sext to i32 = -32768 (0xFFFF8000). opt constant-folds `sext(shl nsw i16 -1, 15)` to exactly -32768, confirming the input is defined and the correct result is -32768. The emitted PTX is `mul.wide.s16 %r, %rs, -32768`, which for %rs = -1 computes (-1)*(-32768) = +32768 (0x00008000). -32768 != +32768 -> miscompile. The i32->i64 case (ShiftAmt 31) is identical: emitted `mul.wide.s32 %rd, %r, -2147483648`. The ZERO_EXTEND path is fine (mul.wide.u treats the constant as unsigned). The older TryMULWIDECombine avoids this because it builds the constant at full width and rejects it via Val.isSignedIntN(OptSize) (32768 doesn't fit signed i16). The fix is to bail out (or use the unsigned widening, but signedness is fixed by the extend) when 1<<ShiftAmt does not fit as a signed value in DemotedVT, i.e. when ShiftAmt >= FromVT.bits-1 for the signed case.

## Trigger

nvptx64, any sm/ptx, -O1+ (OptLevel != None). Pattern: sext i32 (shl nsw i16 x, 15) to i32, or sext i64 (shl nsw i32 x, 31) to i64. Triggers for the defined input x = -1 (and any x where the shl-nsw is non-poison, e.g. x=-1). Reached via PerformDAGCombine ISD::SIGN_EXTEND -> combineMulWide (line 7100).

## Reproducer

See `repro.ll` / `cmd.sh`.

```
define i32 @sext_shl15(i16 %x) {
  %s = shl nsw i16 %x, 15
  %e = sext i16 %s to i32
  ret i32 %e
}

define i64 @sext_shl31(i32 %x) {
  %s = shl nsw i32 %x, 31
  %e = sext i32 %s to i64
  ret i64 %e
}
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_90 -O2 -o - repro.ll
```

## Observed (wrong) output

```
// sext_shl15:
	ld.param.b16 	%rs1, [sext_shl15_param_0];
	mul.wide.s16 	%r1, %rs1, -32768;
	st.param.b32 	[func_retval0], %r1;
	ret;
// sext_shl31:
	ld.param.b32 	%r1, [sext_shl31_param_0];
	mul.wide.s32 	%rd1, %r1, -2147483648;
	st.param.b64 	[func_retval0], %rd1;
	ret;

For input x = -1: mul.wide.s16(-1, -32768) = (-1)*(-32768) = +32768 (0x00008000), and mul.wide.s32(-1, -2147483648) = +2147483648.
```

## Expected

sext_shl15(-1) should return -32768 (0xFFFF8000); opt constant-folds sext(shl nsw i16 -1, 15) to -32768. sext_shl31(-1) should return -2147483648. A correct lowering is the multiplier as a POSITIVE wide constant, e.g. equivalent to sext(x) then shl by 15, which llc emits correctly for the reference IR `mul i32 (sext x), 32768` as `ld.param.s16; shl.b32 %r, %r, 15`. The emitted negative immediates (-32768, -2147483648) into mul.wide.s flip the product's sign for negative x, so the PTX produces +32768 / +2147483648 instead of -32768 / -2147483648.

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.92, verify confidence 0.98).

> Confirmed real miscompile. Mechanism verified at NVPTXISelLowering.cpp:6380-6385: combineMulWide rewrites (sign_extend (shl nsw x, ShiftAmt)) into NVPTXISD::MUL_WIDE_SIGNED(x, 1<<ShiftAmt), building the multiplier constant in the narrow type FromVT via APInt(FromVT.getSizeInBits(), 1) << ShiftAmt. When ShiftAmt == FromVT.bits-1 (15 for i16, 31 for i32), the constant's sign bit is set: 1<<15 = 0x8000, emitted into PTX as the signed value -32768. mul.wide.s16/s32 sign-extend BOTH operands, so the immediate is interpreted as -32768, not the intended +32768.

Concrete defined input x = -1 (i16 0xFFFF): IR semantics of sext(shl nsw i16 -1, 15) = -32768 (0xFFFF8000). This input is genuinely non-poison: -1<<15 mathematically = -32768, which fits in signed i16 so nsw is satisfied; opt -passes=sccp,instcombine,instsimplify folds it to -32768. The emitted PTX `mul.wide.s16 %r1, %rs1, -32768` computes (-1)*(-32768) = +32768 (0x00008000). -32768 != +32768 -> wrong sign, wrong value. The i32->i64 case (shl 31 -> mul.wide.s32 with -2147483648) is identical: for x=-1 it yields +2147483648 vs correct -2147483648.

PTX mul.wide.s16/s32 semantics confirmed (NVIDIA PTX ISA section 9.7.1.3 / general spec): signed n-bit operands sign-extended to full 2n-bit product. The immediate -32768 is the signed 16-bit value 0x8000, sign-extended to 0xFFFF8000.

Adversarial checks all pass: (1) input is defined/non-poison (no UB, no poison); (2) the ZERO_EXTEND path is fine since mul.wide.u treats the consta
