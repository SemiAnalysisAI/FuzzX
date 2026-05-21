# w45: X86AsmPrinter `%P` modifier: AT&T path ignores "disp-only", emits base/index that Intel correctly suppresses

File: `llvm/lib/Target/X86/X86AsmPrinter.cpp`

## Summary

Inline-asm modifier `%P` on a memory operand is implemented in
`PrintAsmMemoryOperand` (line ~891) by passing the magic Modifier string
`"disp-only"` to either `PrintIntelMemReference` or `PrintMemReference`.

`PrintIntelMemReference` honors `"disp-only"` (lines 542-544) and forces
`HasBaseReg = false` so that a memory operand referencing a global with a
base/index register prints only the displacement.

`PrintMemReference` (line 516) delegates to `PrintLeaMemReference`, which
**only inspects the modifier values `"no-rip"` and `"H"`** (lines 433, 454).
It silently ignores `"disp-only"`, so the AT&T path falls through to the
normal mem-operand printer that emits `(base,index,scale)` parens.

Result: for the same `${0:P}` inline-asm token on the same memory operand
that has a base or index register plus a global displacement, Intel prints
`globalsym` while AT&T prints `globalsym(%rax)` (or similar). The comment
above `case 'P'` even says "Print memory only with displacement", which is
what only Intel does.

## Code

`X86AsmPrinter.cpp` line ~525 (`PrintLeaMemReference` signature accepts a
Modifier, but the only switch arms are `"no-rip"` and `"H"`):

```cpp
// If we really don't want to print out (rip), don't.
bool HasBaseReg = BaseReg.getReg() != 0;
if (HasBaseReg && Modifier == "no-rip" && BaseReg.getReg() == X86::RIP)
  HasBaseReg = false;
...
if (Modifier == "H")
  O << "+8";
```

vs. `PrintIntelMemReference` line 542-544:

```cpp
if ((DispSpec.isGlobal() || DispSpec.isSymbol()) && Modifier == "disp-only") {
  HasBaseReg = false;
}
```

## Why it's a bug pattern match

"Asm printer that emits the wrong constraint modifier for inline-asm" —
the AT&T arm of the `P` constraint modifier path does not suppress
base/index registers as documented and as the Intel arm does.

## Severity

Inline-asm output for AT&T users of `%P` with a register-laden memory
operand silently includes the regs the user explicitly asked to be stripped.
For users who relied on this to feed a symbol into a directive expecting
only a label (e.g. `.long ${0:P}`), the assembler rejects or mis-relocates
the output.
