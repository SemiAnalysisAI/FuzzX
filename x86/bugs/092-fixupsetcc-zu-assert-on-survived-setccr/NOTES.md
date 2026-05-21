# X86FixupSetCC: SETCCr reaching ZU path triggers assertion failure

## File / lines
`llvm/lib/Target/X86/X86FixupSetCC.cpp`, lines 80, 120-125.

## Reasoning
`fixupSetCC` accepts both `X86::SETCCr` and `X86::SETZUCCr` at the filter (line 80).
Later, when `ST->hasZU()` and `!ST->preferLegacySetCC()`, the code asserts that
`MI` is already `SETZUCCr`:
```cpp
if (ST->hasZU()) {
  if (!ST->preferLegacySetCC())
    assert((MI.getOpcode() == X86::SETZUCCr) &&
           "Expect setzucc instruction!");
  else
    MI.setDesc(TII->get(X86::SETZUCCr));
  ...
}
```
A SETCCr can survive to this pass even on a ZU-capable target. **Concretely**,
`llvm/lib/Target/X86/GISel/X86InstructionSelector.cpp` at lines 1197 and 1300 emits
`BuildMI(..., TII.get(X86::SETCCr), ResultReg).addImm(CC)` *unconditionally* —
no `hasZU/preferLegacySetCC` gate. If GISel is used (`-global-isel`) on a ZU
subtarget and the SETCCr feeds a `MOVZX32rr8`, the filter at line 80 of
`X86FixupSetCC.cpp` accepts the SETCCr and the assert at line 122 fires:
`"Expect setzucc instruction!"`. In release builds this is undefined behavior;
in asserts it kills the compiler. Compare with `X86FastISel.cpp` line 1438 and
`X86FlagsCopyLowering.cpp` line 756 which both correctly gate the opcode choice
on `(!hasZU || preferLegacySetCC) ? SETCCr : SETZUCCr`.

The safe fix is to do `MI.setDesc(TII->get(SETZUCCr))` unconditionally when `hasZU()`
(matching the `preferLegacySetCC` branch).

## Candidate MIR / IR
Construct an MIR test on a ZU-capable subtarget (e.g. `-mattr=+zu` once available)
where a `SETCCr` survives into the late pipeline followed by a `MOVZX32rr8`:

```mir
# -mtriple=x86_64-- -mattr=+zu -run-pass=x86-fixup-setcc
---
name: foo
body:
  bb.0:
    CMP64rr $rdi, $rsi, implicit-def $eflags
    $al = SETCCr 4, implicit $eflags    ; SETCCr, not SETZUCCr
    $eax = MOVZX32rr8 killed $al
    RET 0, $eax
```

## Wrong outcome
Assertion failure (`"Expect setzucc instruction!"`) or, in release builds, the new
SETZUCCr semantics are silently applied to an instruction that the rest of the IR
still believes is a partial-write SETCCr — diverging dependency model for the
upper bits.
