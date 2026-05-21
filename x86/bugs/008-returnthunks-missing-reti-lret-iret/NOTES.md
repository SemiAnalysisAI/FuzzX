# 008 — X86ReturnThunks ignores `retl $N` / `iret*` / `lret*` (Retbleed mitigation gap)

Component: X86ReturnThunks

## Source

`llvm/lib/Target/X86/X86ReturnThunks.cpp:73-80`

```cpp
const bool Is64Bit = ST.getTargetTriple().isX86_64();
const unsigned RetOpc = Is64Bit ? X86::RET64 : X86::RET32;
...
for (MachineInstr &Term : MBB.terminators())
  if (Term.getOpcode() == RetOpc)
    Rets.push_back(&Term);
```

The pass exists to implement the kernel's `-mfunction-return=thunk-extern`
mitigation (function attribute `fn_ret_thunk_extern`): every ret in the
function must become a `jmp __x86_return_thunk`. The opcode filter is too
narrow — it ignores:

- `RETI32` / `RETI64` (`ret $N`) — emitted on x86 32-bit stdcall / fastcall
  callees, and from inline asm `ret $8`.
- `LRET32` / `LRET64` / `LRETI32` / `LRETI64` — far returns (inline asm in
  kernel entry/exit).
- `IRET32` / `IRET64` — interrupt returns (kernel entry/exit).

Any of these survive the pass and remain bare returns, defeating the
mitigation the user explicitly opted into.

The companion `CS_PREFIX` insertion branch immediately above shares the
same single-opcode narrowing.

## Demonstration (stdcall callee)

`repro.ll` declares an `x86_stdcallcc` function with `fn_ret_thunk_extern`.
`./cmd.sh` shows:

```
foo:
    movl    4(%esp), %eax
    retl    $4              ; <-- should be: jmp __x86_return_thunk
```

The `retl $4` (RETI32) is left in place — the mitigation contract is
silently violated.

## Fix

Replace the single-opcode match with `Term.isReturn()` (the `Return = 1`
TableGen flag covers the entire family) and add `CS` prefixes likewise.
