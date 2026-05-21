# X86::optimizeInstFromVEX3ToVEX2 swap doesn't reject XMM16-31 / EGPR operands

## File / lines

`llvm/lib/Target/X86/MCTargetDesc/X86EncodingOptimization.cpp`, lines 23-101
(function `X86::optimizeInstFromVEX3ToVEX2`)

## Pattern

Wrong VEX2-vs-VEX3 selection criterion.

## What the code does

This function attempts to convert a 3-byte VEX-encoded instruction into a
2-byte VEX form by either commuting operands (for commutable MRMSrcReg ops)
or rewriting to a `_REV` opcode (for `vmovaps`-class moves). The decision to
swap is made by the final check at lines 93-95:

```cpp
if (X86II::isX86_64ExtendedReg(MI.getOperand(OpIdx1).getReg()) ||
    !X86II::isX86_64ExtendedReg(MI.getOperand(OpIdx2).getReg()))
  return false;
```

i.e. require OpIdx1 to be a "low" reg (e.g. XMM0-7) and OpIdx2 to be
"extended" (per `isX86_64ExtendedReg`). After the swap, the extended reg ends
up in the position that uses VEX.R (encodable in 2-byte VEX) rather than
VEX.B (which would force 3-byte VEX).

## The bug

`X86II::isX86_64ExtendedReg` (X86BaseInfo.h:1193-1255) returns true for **both**:

- legacy "extended" regs XMM8-15 / YMM8-15 / R8-R15 — encodable in VEX (1 extra bit)
- EVEX-only regs XMM16-31 / YMM16-31 / ZMM16-31 / R16-R31 — NOT encodable in any VEX form

The check does not distinguish these two classes. If `OpIdx2` is an EVEX-only
register (XMM/YMM 16-31 or R16-R31), the function happily swaps the operands
or rewrites to the `_REV` opcode, and downstream `emitVEXOpcodePrefix` /
`determineOptimalKind` will then try to emit the instruction as VEX (2- or
3-byte). VEX has only one high-bit per register field, so the EGPR/EVR
register cannot be represented and the emitted instruction will silently
encode the wrong register (the low 4 bits of XMM16 = XMM0, etc.).

Concretely, for a default-case commutable VEX MRMSrcReg three-operand
instruction with operands `(dst, src1=XMM0, src2=XMM16)` the check passes
(OpIdx1=XMM0 not extended, OpIdx2=XMM16 extended), the operands are swapped
to `(dst, XMM16, XMM0)`, and the encoder then emits XMM16 as the .vvvv field
without setting V'/V4 — which is impossible in VEX. The resulting bytes
decode as a reference to XMM0.

The same bug exists in the `_REV` branch (lines 73-89): swapping
`(dst=XMM0, src=XMM20)` to a `*_REV` form puts XMM20 into the .reg field but
the encoding is still VEX, losing the high bit.

## Why it may not have been observed

In practice the X86 ISel never selects a VEX-encoded opcode when an EGPR/EVR
operand is required — it would have selected the EVEX form. So the input MI
is normally well-typed and this corner of the check never fires in
compiler-generated code.

However:

1. This function is also called from the assembler (`X86AsmParser.cpp:3883`),
   so hand-written `.s` that constructs an MI with mixed legacy and EVEX-only
   regs (or assembler internal paths that try matching VEX-form aliases for
   EVEX regs) could hit it. The MCParser side should reject EVEX-only regs
   against a VEX opcode at parse time, but this is defense-in-depth.
2. Future codegen changes (e.g. CompressEVEX adding new conversions) could
   easily produce a VEX MI with an EVEX-only operand and would silently
   miscompile.

## Suggested fix

Replace `isX86_64ExtendedReg` with a stricter predicate that only accepts
registers encodable in VEX (XMM/YMM 8-15, R8-R15) and rejects the EVEX-only
ones (XMM/YMM/ZMM 16-31, R16-R31, i.e. `isApxExtendedReg` plus the high vector
half). E.g.:

```cpp
auto isVEXExtendedReg = [](MCRegister R) {
  return X86II::isX86_64ExtendedReg(R) && !isEVEXOnlyReg(R);
};
if (isVEXExtendedReg(op1) || !isVEXExtendedReg(op2))
  return false;
```

Or, more conservatively, bail entirely if either operand is EVEX-only.
