# c003 — combineMulWide: sext(shl nsw x, topbit) -> mul.wide.s with negative constant miscompiles

- region: L01-combines-mul-shl
- file: NVPTXISelLowering.cpp 6379-6385
- kind: miscompile
- confidence(finder): 0.92

## Mechanism
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

## IR
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

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_90 -O2`
