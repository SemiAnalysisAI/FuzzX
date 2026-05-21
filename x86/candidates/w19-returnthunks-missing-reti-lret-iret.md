# X86ReturnThunks: only rewrites RET64/RET32, misses RETI*/LRET*/IRET*

File: llvm/lib/Target/X86/X86ReturnThunks.cpp:73-80

```
const bool Is64Bit = ST.getTargetTriple().isX86_64();
const unsigned RetOpc = Is64Bit ? X86::RET64 : X86::RET32;
...
for (MachineInstr &Term : MBB.terminators())
  if (Term.getOpcode() == RetOpc)
    Rets.push_back(&Term);
```

## Reasoning

`-mfunction-return=thunk-extern` / `fn_ret_thunk_extern` is the kernel's
Retbleed mitigation: every ret-class instruction in the function must be
converted into `jmp __x86_return_thunk`. The pass narrowly matches a
single opcode (`RET64` in 64-bit mode, `RET32` in 32-bit mode), so any
other ret variant survives unmitigated:

- `RETI32` / `RETI64` — `ret imm16`, emitted whenever the callee pops
  a stack adjustment (x86 32-bit stdcall / fastcall, or inline asm).
  Confirmed reproducible — see below.
- `LRET32` / `LRET64` / `LRETI32` / `LRETI64` — far returns; usable
  from inline asm in kernel low-level entry/exit code, which is
  exactly the audience for `thunk-extern`.
- `IRET32` / `IRET64` — interrupt returns; same audience.

For `RETI32`/`RETI64` the omission is especially bad because a
*plain* stdcall callee in a kernel-style module compiled with
`-mfunction-return=thunk-extern` will keep its raw `ret $N`
instruction unmodified, defeating Retbleed mitigation that the user
explicitly asked for. The pass should iterate `MBB.terminators()` and
match on `MI.isReturn()` (the property is set in TableGen for the
whole family) instead of a single opcode.

The CS_PREFIX branch immediately above has the same single-opcode
narrowing bug.

## IR repro sketch

```
; llc -mtriple=i686-unknown-linux-gnu reduce.ll  →  retl $4 (un-thunked)
target triple = "i686-unknown-linux-gnu"
define x86_stdcallcc i32 @foo(i32 %x) #0 {
  ret i32 %x
}
attributes #0 = { fn_ret_thunk_extern }
```

Observed output (verified locally):
```
foo:
  movl 4(%esp), %eax
  retl $4              ; <-- should be: jmp __x86_return_thunk
```

For x86_64 the same shape via inline asm:
```
define void @bar() #0 {
  call void asm sideeffect "ret $$8", ""()
  unreachable
}
attributes #0 = { fn_ret_thunk_extern }
```
The `RETI64` in the MI stream is not rewritten.

## Expected wrong outcome

A function the user annotated `fn_ret_thunk_extern` retains a bare
`ret $N` (or `iretq`, `lretq`) terminator, leaving a Retbleed-vulnerable
return that silently violates the contract advertised by
`-mfunction-return=thunk-extern`.
