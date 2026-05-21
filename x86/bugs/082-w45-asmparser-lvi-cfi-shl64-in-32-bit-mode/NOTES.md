# w45: X86AsmParser::applyLVICFIMitigation unconditionally emits SHL64mi even in 16/32-bit modes for RET16/RET32/RETI16/RETI32

File: `llvm/lib/Target/X86/AsmParser/X86AsmParser.cpp`
Lines: 4125-4151

## Summary

`applyLVICFIMitigation` mitigates the Load-Value-Injection vector around
`RET*` instructions by prepending `SHL [<stack>], 0` and `LFENCE`. The
switch covers `RET16`, `RET32`, `RET64`, `RETI16`, `RETI32`, `RETI64`. The
emitted shift opcode is hard-coded to `X86::SHL64mi`:

```cpp
MCRegister Basereg =
    is64BitMode() ? X86::RSP : (Parse32 ? X86::ESP : X86::SP);
const MCExpr *Disp = MCConstantExpr::create(0, getContext());
auto ShlMemOp = X86Operand::CreateMem(getPointerWidth(), /*SegReg=*/0, Disp,
                                      /*BaseReg=*/Basereg, /*IndexReg=*/0,
                                      /*Scale=*/1, SMLoc{}, SMLoc{}, 0);
ShlInst.setOpcode(X86::SHL64mi);
ShlMemOp->addMemOperands(ShlInst, 5);
ShlInst.addOperand(MCOperand::createImm(0));
```

In 32-bit mode the base register is `%esp` and `Basereg` is correctly
chosen, but the opcode is still `SHL64mi`. Encoding `SHL64mi` requires a
REX.W prefix, which is invalid in 32-bit/16-bit mode (REX prefixes are
undefined outside long mode). The assembler will happily produce these
bytes; the resulting object file contains an unencodable instruction for
the target mode.

For 16-bit mode the situation is worse: `Basereg=X86::SP`, but a 64-bit
shift with a 16-bit address register makes no sense and the encoding
emitted will be 16-bit-mode bytes for what was meant to be a stack
hardening sequence.

## Why it's a bug pattern match

"Asm parser that accepts an out-of-range/invalid encoding" —
the parser (running with `-mlvi-cfi`) accepts `ret`/`retw`/`retl` in
non-64-bit mode and emits an instruction with REX.W set in a mode that
forbids REX. The downstream assembler emission silently produces invalid
bytes.

## Fix sketch

Choose the shift opcode based on mode:

```cpp
unsigned Opc = is64BitMode() ? X86::SHL64mi
             : is32BitMode() ? X86::SHL32mi
                             : X86::SHL16mi;
ShlInst.setOpcode(Opc);
```

(or skip the mitigation in non-64-bit mode entirely, since LVI was
originally documented as a 64-bit issue).
