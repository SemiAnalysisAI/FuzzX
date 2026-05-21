# w45: X86AsmPrinter::PrintAsmOperand emits AT&T syntax for `%a` / `%A` modifiers regardless of inline-asm dialect

File: `llvm/lib/Target/X86/X86AsmPrinter.cpp`
Lines: 764-785 ('a'), 805-811 ('A')

## Summary

`PrintAsmOperand` implements the GCC-style operand modifiers `%a` ("address")
and `%A` ("indirect via register") with hard-coded AT&T punctuation. The
inline-asm dialect (`MI->getInlineAsmDialect()`) is never consulted in either
case, so an inline asm string written for Intel syntax that uses these
modifiers will produce syntactically invalid output.

## Code

```cpp
case 'a':
  ...
  case MachineOperand::MO_GlobalAddress:
    PrintSymbolOperand(MO, O);
    if (Subtarget->is64Bit())
      O << "(%rip)";          // AT&T-only
    return false;
  case MachineOperand::MO_Register:
    O << '(';                 // AT&T parens
    PrintOperand(MI, OpNo, O);// PrintOperand already prefixes '%' or not based on dialect
    O << ')';
    return false;
  }

case 'A':
  if (MO.isReg()) {
    O << '*';                 // AT&T "call *" indirection marker
    PrintOperand(MI, OpNo, O);
    return false;
  }
```

Intel syntax counterparts would be:

  - `%a` for a 64-bit global: `[rip + globalsym]`, not `globalsym(%rip)`.
  - `%a` for a register: `[reg]`, not `(reg)` / `(%reg)`.
  - `%A` for indirect call: Intel uses no marker (`call rax`), not `*rax`.

Compare with `case 'c'` (line 787) which deliberately *omits* the AT&T `$`
prefix and is dialect-neutral — the surrounding code does know that dialect
matters, this case just forgot.

## Why it is a bug pattern match

"Asm printer that emits the wrong constraint modifier for inline-asm" —
the wrong syntactic form is emitted for the constraint modifier when the
inline asm dialect is Intel.

## Repro sketch

```ll
@g = global i32 0
define i32 @f() {
  call void asm sideeffect inteldialect "lea rax, $a0", "*m,~{rax}"(ptr elementtype(i32) @g)
  ret i32 0
}
```
Compile with `llc -x86-asm-syntax=intel`. The `${0:a}` mod (or equivalently
the bare `*m` constraint with `a` modifier) ends up calling
`X86AsmPrinter::PrintAsmOperand` which emits `(g)(%rip)` style text into an
Intel-syntax stream.

(Note: triggering this through the IR-level inline-asm path requires the
constraint string to actually route a single non-mem operand through the
`PrintAsmOperand` path; the inline-asm machinery passes through this code
whenever `${N:a}` or `${N:A}` is encountered in the asm template.)

## Severity

Generates assembler-rejected (or silently mis-assembled) text for users of
Intel-dialect inline asm using these well-documented modifiers.
