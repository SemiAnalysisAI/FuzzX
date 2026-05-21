## CET-IBT omits ENDBR after indirect `returns_twice` call site

`llvm/lib/Target/X86/X86IndirectBranchTracking.cpp:87-95` (`IsCallReturnTwice`),
`llvm/lib/Target/X86/X86IndirectBranchTracking.cpp:146-151` (call-site scan).

NON-DEFAULT pass (requires `-fcf-protection=branch`).

```cpp
static bool IsCallReturnTwice(llvm::MachineOperand &MOp) {
  if (!MOp.isGlobal())                          // <-- bails on indirect calls
    return false;
  auto *CalleeFn = dyn_cast<Function>(MOp.getGlobal());
  if (!CalleeFn)                                // <-- bails on aliases / ifuncs
    return false;
  AttributeList Attrs = CalleeFn->getAttributes();
  return Attrs.hasFnAttr(Attribute::ReturnsTwice);  // <-- only checks fn attr,
                                                    //     not call-site attr
}
```

`IsCallReturnTwice` only recognizes a returns-twice call if the operand-0 is a
plain `Function*` whose Function-level attributes include `returns_twice`. As a
result the pass misses every:

1. **Indirect** call where the IR call-site carries `returns_twice` (e.g., a
   call through a function pointer obtained at runtime, or a vtable slot).
2. Direct call whose target is a `GlobalAlias`/`GlobalIFunc` resolving to a
   returns-twice function.
3. Direct call whose call-site has `returns_twice` but whose callee `Function`
   does not (perfectly legal IR: `call i32 %fp(...) #1` where `#1 =
   returns_twice`).

In all three cases, `addENDBR(MBB, std::next(I))` (line 149) is never executed,
so the instruction immediately after the call has no `endbr64`. When the
runtime longjmp/SS_resume actually returns the second time, the CPU's IBT
state-machine raises `#CP` (Control-Protection fault) on the next indirect
landing because it sees no ENDBR. The process dies on the recovery path even
though everything was statically known.

### Candidate IR (`/tmp/x86bugs/ibt_indirect_rt.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @foo(ptr %fp, ptr %buf) {
entry:
  %r = call i32 %fp(ptr %buf) #1                ; <-- returns_twice at call site
  %is_jmp = icmp ne i32 %r, 0
  br i1 %is_jmp, label %recovered, label %normal
recovered:
  ret i32 1
normal:
  ret i32 0
}

attributes #1 = { returns_twice }

!llvm.module.flags = !{!0}
!0 = !{i32 4, !"cf-protection-branch", i32 1}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu` output (abridged):

```
foo:
        endbr64
        ...
        callq   *%rax           # <-- indirect returns_twice
        testl   %eax, %eax      # <-- NO ENDBR HERE; on recovery, #CP
        je      .LBB0_2
...
```

Contrast with the direct-call form (`callq setjmp@PLT`), which correctly
emits `endbr64` immediately after the call.

### Fix sketch

Detect returns-twice from either the call instruction's flags or the callee's
attributes:

```cpp
// Need access to the MachineInstr to check call-site attrs via getCallTarget /
// MachineInstr::getFlags, or pre-stamp ReturnsTwice on the MI in ISel.
bool isReturnsTwiceCall(const MachineInstr &MI) {
  if (MI.getFlag(MachineInstr::FrameSetup)) /* ... */;
  // Inspect MachineInstr::AdditionalCallInfo / the call's IR call-site
  // attribute (via MF.getAdditionalCallSiteInfo) for ReturnsTwice.
}
```

At minimum `IsCallReturnTwice` should also pierce a `GlobalAlias`/`IFunc`:
```cpp
if (auto *GA = dyn_cast<GlobalAlias>(MOp.getGlobal()))
  return ...->getAliasee()->...;
```

### Why this matters

Setjmp-style functions invoked through a function pointer are common in
high-level runtimes (libuv, Boehm GC, GHC RTS, Tcl, Lua coroutines). Compiling
the host with `-fcf-protection=branch` on a CET-capable CPU yields a working
binary that crashes only on the second (longjmp) return.
