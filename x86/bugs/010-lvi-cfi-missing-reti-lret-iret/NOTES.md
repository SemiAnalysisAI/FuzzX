# 010 — LVI ret-hardening misses `RETI64` / `LRET64` / `IRET64`

Component: X86LoadValueInjectionRetHardening

## Triage note

Keep this recorded, but do not prioritize it for the current fix batch. It only
affects the non-default `-mlvi-cfi` / `+lvi-cfi` mitigation path, and the
observed behavior is a missing hardening transform rather than a general x86
correctness miscompile.

## Source

`llvm/lib/Target/X86/X86LoadValueInjectionRetHardening.cpp:72-74`

```cpp
for (auto MBBI = MBB.begin(); MBBI != MBB.end(); ++MBBI) {
  if (MBBI->getOpcode() != X86::RET64)
    continue;
  ...
}
```

The mitigation must convert every ret-class terminator to a
`pop/lfence/jmp *reg` sequence so an attacker cannot poison the speculative
return target. The pass only matches `X86::RET64`, silently skipping:

- `X86::RETI64`  — `ret imm16`, emitted by inline asm `ret $$8`, stdcall-like
  thunks, certain Windows-ABI returns.
- `X86::LRET64` / `X86::LRETI64` — far returns (kernel ring transitions).
- `X86::IRET64` — interrupt return (kernel).

For any of these the pass increments `NumFunctionsConsidered` but inserts
zero fences, leaving an un-hardened return in a function the user asked
to be LVI-CFI hardened.

This is the LVI-pass mirror of bug 008 (X86ReturnThunks) — same single-opcode
narrowing, different mitigation.

## Demonstration

```
$ ./cmd.sh
===== llc with -mattr=+lvi-cfi (expect: pop/lfence/jmp; observed: bare retq $8) =====
f:
        #APP
        retq    $8        ; <-- not hardened
        #NO_APP
.Lfunc_end0:
```

Compare with a function ending in plain `ret`: the LVI pass correctly rewrites
it to `popq %rcx; lfence; jmpq *%rcx`.

## Fix

Replace `MBBI->getOpcode() != X86::RET64` with `!MBBI->isReturn()` (the
`isReturn = 1` TableGen flag covers the whole family) and add handling
for the variants — at minimum, refuse to compile rather than silently
omitting the fence.
