# LVI Ret-Hardening: only handles X86::RET64, misses RETI64/LRET64/IRET64

File: llvm/lib/Target/X86/X86LoadValueInjectionRetHardening.cpp:72-74

```
for (auto MBBI = MBB.begin(); MBBI != MBB.end(); ++MBBI) {
  if (MBBI->getOpcode() != X86::RET64)
    continue;
  ...
```

## Reasoning

LVI ret-hardening must replace every `ret`-class instruction at the end
of a mitigated function with the `pop/lfence/jmp *reg` sequence, so that
the speculative target of the popped return address cannot be poisoned
by an attacker-controlled memory value (the classic LVI gadget against
indirect predictors). The pass only checks `MBBI->getOpcode() != X86::RET64`,
which misses every other ret variant the x86 backend can emit in 64-bit
mode:

- `X86::RETI64`  - `ret imm16` (callee pops stack adjustment; used by some
  thunk lowerings and ABI variants — e.g. functions returning aggregates in
  Windows x64 conventions, or hand-written .s injected via `__asm__`).
- `X86::LRET64` / `X86::LRETI64` - far returns (rare but legal; used when
  switching CS — kernel exits, far-call thunks).
- `X86::IRET64` - interrupt return (kernel mode, but the pass is enabled
  whenever `useLVIControlFlowIntegrity()` and `is64Bit()`; the user can
  enable lvi-cfi in a kernel module).
- `X86::TCRETURNri64`/`TCRETURNmi64` after pseudo-lowering produce
  `TAILJMPr64` / `TAILJMPm64`, which become `jmp *reg` / `jmp *[mem]` —
  arguably already lfenced via the load-hardening path, but a tail-call
  to memory bypasses the ret rewrite entirely.

For RETI64 in particular: a function compiled with `-mlvi-cfi` that
ends with a stack-adjusting return (e.g. inline asm containing `ret 8`,
or an MS-ABI thunk) will retain the raw `ret 8` instruction with no
lfence in front, defeating the mitigation that this pass is documented
to provide.

The fix is to switch on `MBBI->isReturn()` (which is set in TableGen via
`isReturn = 1` for RET64/RETI64/LRET*/IRET*) and either handle each
variant or assert on the unsupported ones, rather than silently skipping
them.

## IR repro sketch

```
; llc -mtriple=x86_64-linux-gnu -mattr=+lvi-cfi reduce.ll
define void @f() {
  call void asm sideeffect "ret $$8", ""()
  unreachable
}
```

The inline-asm `ret 8` survives as `RETI64` in the MI stream and is
ignored by `runX86LoadValueInjectionRetHardening`; the resulting assembly
contains a bare `retq $8` with no preceding `lfence`, contradicting the
pass's stated guarantee.

## Expected wrong outcome

The function is counted in `NumFunctionsConsidered` but `NumFences`
remains zero — i.e. the pass silently emits an un-hardened ret-class
instruction in a function the user explicitly requested be LVI-CFI
hardened.
