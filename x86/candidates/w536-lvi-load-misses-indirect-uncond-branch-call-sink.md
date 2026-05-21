## LVI load hardening: unconditional indirect branch / call NOT modeled as sink

`llvm/lib/Target/X86/X86LoadValueInjectionLoadHardening.cpp:786-794`
(`instrUsesRegToBranch`).

NON-DEFAULT pass (requires `-mllvm -x86-lvi-load` or
`"target-features"="+lvi-load-hardening"`).

```cpp
bool X86LoadValueInjectionLoadHardeningImpl::instrUsesRegToBranch(
    const MachineInstr &MI, Register Reg) const {
  if (!MI.isConditionalBranch())                        // <-- only cond branches
    return false;
  for (const MachineOperand &Use : MI.uses())
    if (Use.isReg() && Use.getReg() == Reg)
      return true;
  return false;
}
```

`MachineInstr::isConditionalBranch()` is defined as
`isBranch() && !isBarrier() && !isIndirectBranch()`
(`llvm/include/llvm/CodeGen/MachineInstr.h:1018-1020`), which explicitly
**excludes**:

- `JMP64r` / `JMP32r` (unconditional indirect jump, used by jump tables and
  computed gotos / `indirectbr`)
- `JMP64m` / `JMP32m` (load-folded indirect jump)
- Indirect calls (which are not even classified as branches by MI)

There is also no `instrUsesRegToCall` companion check. The result: a loaded
pointer that is consumed by an unconditional indirect branch or by an indirect
call (with the load *not* fused into the branch/call by the MI) creates a
SOURCE+SINK pair that the gadget analyzer does **not** detect, so **no
`LFENCE` is inserted** between the secret-tainted load and the speculative
disclosure.

This is exactly the spectre-v1 / LVI shape the pass was written to mitigate:
the architectural load result is forwarded to a control-flow instruction whose
target leaks bits of the load through the speculation window.

### Candidate IR (`/tmp/x86bugs/lvi_jmp_pure2.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
@globaltarget = external global ptr

define void @pure_jmp() "target-features"="+lvi-load-hardening" {
entry:
  %v = load ptr, ptr @globaltarget        ; SOURCE (loaded pointer)
  %vint = ptrtoint ptr %v to i64
  %vmasked = and i64 %vint, -2            ; defeat load-jmp fusion
  %vptr = inttoptr i64 %vmasked to ptr
  indirectbr ptr %vptr, [label %B1, label %B2]   ; UNCONDITIONAL indirect sink
B1: ret void
B2: ret void
}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu`:

```
pure_jmp:
        movq    globaltarget@GOTPCREL(%rip), %rax
        lfence                          # for GOT-load (its addr came from arg)
        movq    (%rax), %rax            # SOURCE
        andq    $-2, %rax
        jmpq    *%rax                   # SINK -- no LFENCE in between
```

Note the `lfence` between the GOT load and the table load is correctly emitted
(the GOT load result is used as an address in the next load, hitting
`instrUsesRegToAccessMemory`). But there is no `lfence` between the *value
load* and the `jmpq *%rax` that consumes the loaded pointer as the jump
target.

Same shape repros with `call void %vptr()` -> `callq *%rax`.

### Compare to spectre-v1 reasoning

In LVI the architectural value of `%rax` after `(%rax)` may be the attacker's
poisoned value; even though `andq $-2` is architectural, the CPU may
speculatively execute the `jmpq *%rax` with a speculated `%rax` and disclose
secrets at the speculatively-fetched target. The whole point of LVI load
hardening is to insert `lfence` along that path. The check at line 788 is the
gate that lets this through.

### Fix sketch

```cpp
bool isControlFlowSink(const MachineInstr &MI) const {
  // Conditional branches.
  if (MI.isConditionalBranch())
    return true;
  // Indirect branches and calls of any kind also speculatively expose
  // the targeted address to the front-end.
  if (MI.isIndirectBranch())
    return true;
  if (MI.isCall() && /* indirect */)
    return true;
  return false;
}
```

(Or treat the existing `instrUsesRegToBranch` as the general "register feeds
control-flow target" predicate and drop the `isConditionalBranch` short-circuit.)

LVI control-flow integrity (`-mlvi-cfi`) thunks indirect calls separately, so
when *both* are enabled the call form is mitigated. But when LVI load
hardening is enabled without CFI (the documented "load hardening only"
configuration), the indirect-branch case remains an unfenced LVI gadget.
