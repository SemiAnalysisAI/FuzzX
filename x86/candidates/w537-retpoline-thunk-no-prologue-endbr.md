## CET-IBT does not run on retpoline / LVI thunks (no prologue ENDBR)

`llvm/lib/Target/X86/X86IndirectThunks.cpp:101-109` (`X86IndirectThunks`),
combined with the pass-order constraint: `x86-indirect-branch-tracking` runs
BEFORE `x86-indirect-thunks` in the X86 codegen pipeline.

NON-DEFAULT (requires `-mretpoline` (or `+retpoline-indirect-calls`) *and*
`-fcf-protection=branch`).

### Mechanism

`X86IndirectThunks` synthesizes the thunk function as a brand-new `Function`
via `createThunkFunction` (`llvm/include/llvm/CodeGen/IndirectThunks.h`) at
the very late stage of the pipeline, after `X86IndirectBranchTrackingLegacy`
has already run on every existing function. As a result the IBT
`runOnMachineFunction` is never called on the synthesized
`__llvm_retpoline_r11` (or `__llvm_lvi_thunk_r11`), and no `endbr64` is
emitted at the thunk's entry.

`createThunkFunction` uses `LinkOnceODRLinkage` + hidden visibility, i.e. the
thunk is externally visible (non-`hasLocalLinkage()`). Under the IBT pass's
own predicate `needsPrologueENDBR`
(`X86IndirectBranchTracking.cpp:98-112`):

```cpp
default:
  return (F.hasAddressTaken() || !F.hasLocalLinkage());
```

A LinkOnceODR thunk would *have* qualified for prologue ENDBR if the pass
had actually seen it. Because the pass never runs on the thunk MF, the
unconditional emission is skipped.

### Repro (`/tmp/x86bugs/retpoline_ibt.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @caller(ptr %fp) "target-features"="+retpoline-indirect-calls" {
  call void %fp()
  ret void
}

!llvm.module.flags = !{!0}
!0 = !{i32 4, !"cf-protection-branch", i32 1}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu`:

```
caller:
        endbr64                                 # IBT runs on caller -> OK
        ...
        callq   __llvm_retpoline_r11

__llvm_retpoline_r11:
                                                # <-- NO endbr64 here
        callq   .Ltmp0
.LBB1_1:                                        # Block address taken
        pause
        lfence
        jmp     .LBB1_1
.Ltmp0:
        movq    %r11, (%rsp)
        retq
```

### Why this matters in practice

For the *direct* call from `caller`, ENDBR at the thunk entry is not strictly
required (CET-IBT only enforces it on indirect transfers). The bug surfaces
in two real scenarios:

1. **Module-linked retpoline thunk taken as a function pointer.** Kernel
   modules (and shared objects with copy-relocations) can end up with a
   GOT/PLT slot pointing at `__llvm_retpoline_r11`. The relocator's
   indirection turns the entry into an indirect transfer, which then traps
   on `#CP`.
2. **`-mretpoline-external-thunk`** users (kernel) provide a hand-written
   thunk with ENDBR; the LLVM-generated one silently regresses parity.

The `lib/CodeGen/IndirectThunks.h` comment acknowledges that "the thunk
function has to be inserted on behalf of some other function and then
populated on its own 'iteration' later" -- but for IBT specifically there is
no second iteration: by the time the thunk MF is populated, IBT has been
removed from the pipeline schedule for it.

### Fix sketch

Either:

- Schedule `x86-indirect-branch-tracking` *after* `x86-indirect-thunks`
  (re-order in `X86PassConfig::addPreEmitPass2`), OR
- Have `RetpolineThunkInserter::populateThunk` (and `LVIThunkInserter`)
  emit `ENDBR64` itself when
  `MF.getModule()->getModuleFlag("cf-protection-branch")` is set:

```cpp
void RetpolineThunkInserter::populateThunk(MachineFunction &MF) {
  // ...
  Entry->clear();
  if (MF.getMMI().getModule()->getModuleFlag("cf-protection-branch"))
    BuildMI(Entry, DebugLoc(), TII->get(Is64Bit ? X86::ENDBR64 : X86::ENDBR32));
  Entry->addLiveIn(ThunkReg);
  BuildMI(Entry, DebugLoc(), TII->get(CallOpc)).addSym(TargetSym);
  // ...
}
```

The same fix is required for `LVIThunkInserter::populateThunk` (line 81-99 of
`X86IndirectThunks.cpp`).
