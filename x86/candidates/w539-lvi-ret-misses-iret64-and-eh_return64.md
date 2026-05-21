## LVI ret-hardening only matches `RET64`; misses `IRET64`, `EH_RETURN64`, `RETI64`

`llvm/lib/Target/X86/X86LoadValueInjectionRetHardening.cpp:73`
(the entire pass).

NON-DEFAULT (requires `-mlvi-cfi` / `+lvi-cfi` target feature, which sets
`useLVIControlFlowIntegrity()`).

```cpp
for (auto MBBI = MBB.begin(); MBBI != MBB.end(); ++MBBI) {
  if (MBBI->getOpcode() != X86::RET64)     // <-- ONLY plain RET64
    continue;
  // ... pop+lfence+jmp transform ...
}
```

The pass replaces `ret` with `pop r ; lfence ; jmp *r`, which serializes
the speculative load of the return address against transient execution. The
opcode comparison only matches `X86::RET64`. Other instructions that pop
data from the stack and transfer control to the popped value (and are
therefore equally vulnerable to LVI on the loaded return state) are
silently passed through unfenced:

- **`X86::IRET64` (`iretq`)** — used by `x86_intrcc` interrupt handlers.
  IRET pops RIP, CS, RFLAGS, RSP, SS from the stack. An LVI attack on the
  stack at the moment of IRET can speculatively redirect execution at
  ring-0 to an attacker-chosen RIP, *and* speculatively change CS/SS.
- **`X86::EH_RETURN64`** — used by `llvm.eh.return.i64` (libunwind /
  personality functions). The pseudo is later expanded to a `retq` by
  `X86MCInstLower` *after* the LVI-RET pass has already finished, so the
  resulting `retq` is unfenced.
- **`X86::RETI64` (`ret $N`)** — emitted by `X86ExpandPseudo` for the
  generic `X86::RET` pseudo when stack adjustment is non-zero (e.g.,
  callee-pops calling conventions). Pass-order: `x86-expand-pseudo` runs
  *before* `x86-lvi-ret`, so RETI64 reaches LVI-RET but is not matched.
- **`X86::LRET64`, `X86::LRETI64`** — far-returns (rare but valid).

### Repro 1: IRET64 (`/tmp/x86bugs/lvi_ret_x86_64_winapi.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define x86_intrcc void @intr(ptr byval(i8) %frame) "target-features"="+lvi-cfi" {
  ret void
}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu`:

```
intr:
        iretq                          # <-- NO LFENCE; LVI attack on
                                       #     RIP/CS/RFLAGS/RSP/SS popped here
```

Compare: a plain `ret` in the same configuration produces
`popq %r* ; lfence ; jmpq *%r*` (the safe form).

### Repro 2: EH_RETURN64 (`/tmp/x86bugs/lvi_ret_imm_x.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.eh.return.i64(i64, ptr)

define void @ehret(i64 %off, ptr %h) "target-features"="+lvi-cfi" {
  call void @llvm.eh.return.i64(i64 %off, ptr %h)
  unreachable
}
```

After `x86-lvi-ret` the MIR still contains `EH_RETURN64 $rcx`; the pseudo
expander then turns it into a `retq`, but LVI-RET has already finished:

```
ehret:
        ...
        movq    %rcx, %rsp
        retq                            # <-- NO LFENCE; unwinder return
                                        #     uses an attacker-controllable
                                        #     stack pointer
```

EH_RETURN is the most security-relevant case: it lands at the C++/Itanium
personality routine's unwinding target. The stack pointer is set to
`%rcx`, an arbitrary attacker-influenceable value if the unwind metadata
or `_Unwind_Context` was tampered with. The subsequent unfenced `retq`
then speculatively returns into whatever lives at that pointer.

### Fix sketch

```cpp
static bool isReturnNeedingLVIHardening(unsigned Opc) {
  switch (Opc) {
  case X86::RET64:
  case X86::RETI64:
  case X86::IRET64:
  case X86::LRET64:
  case X86::LRETI64:
    return true;
  default:
    return false;
  }
}

// ...
if (!isReturnNeedingLVIHardening(MBBI->getOpcode()))
  continue;
```

For `EH_RETURN64` the fix requires either expanding the pseudo *before*
LVI-RET, or recognizing it explicitly (since the post-LVI expansion
yields a non-hardenable `retq`).

For `IRET64`, the `pop r ; lfence ; jmp *r` rewrite cannot be used as-is
(IRET pops more than just RIP). The kernel-style mitigation is to emit
an `lfence` *before* the IRET (the iret instruction itself remains, but
the speculation barrier prevents the speculatively-loaded popped state
from being used). At minimum:

```cpp
if (Opc == X86::IRET64) {
  BuildMI(MBB, MBBI, DebugLoc(), TII->get(X86::LFENCE));
  continue;
}
```
