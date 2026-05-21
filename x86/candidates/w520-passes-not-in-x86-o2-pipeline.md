# w520: MachinePipeliner and HardwareLoops are not in the X86 -O2 pipeline

## Verdict

Out of scope for x86 -O2 bug hunting. **No candidates produced.**

## Evidence

### MachinePipeliner

`llvm/lib/CodeGen/MachinePipeliner.cpp` is compiled into CodeGen and the pass is initialized
(`llvm/lib/CodeGen/CodeGen.cpp:105 initializeMachinePipelinerPass(Registry);`), but the X86
target never adds it to its pass pipeline. The only `addPass(&MachinePipelinerID)` /
`createMachinePipeliner` references in tree are:

```
llvm/lib/Target/ARM/ARMTargetMachine.cpp:470:      addPass(&MachinePipelinerID);
llvm/lib/Target/Hexagon/HexagonTargetMachine.cpp:470:    addPass(&MachinePipelinerID);
llvm/lib/Target/AArch64/AArch64TargetMachine.cpp:848:    addPass(&MachinePipelinerID);
llvm/lib/Target/RISCV/RISCVTargetMachine.cpp:640:    addPass(&MachinePipelinerID);
llvm/lib/Target/PowerPC/PPCTargetMachine.cpp:535:    addPass(&MachinePipelinerID);
```

No hit under `llvm/lib/Target/X86/`. Also no `enableMachinePipeliner` override in the X86
subtarget (the base virtual returns `true`, but it is never consulted because X86 doesn't
schedule the pass at all).

### HardwareLoops

The source file lives at `llvm/lib/CodeGen/HardwareLoops.cpp` (note: not
`lib/Transforms/Scalar/` as the prompt said — it moved into CodeGen). Targets that add it:

```
llvm/lib/Target/ARM/ARMTargetMachine.cpp:427:    addPass(createHardwareLoopsLegacyPass());
llvm/lib/Target/Hexagon/HexagonTargetMachine.cpp:467:      addPass(createHexagonHardwareLoops());  // Hexagon-specific, not the generic pass
llvm/lib/Target/PowerPC/PPCTargetMachine.cpp:461:    addPass(createHardwareLoopsLegacyPass());
```

No hit under `llvm/lib/Target/X86/`. The pass is initialized in CodeGen but never scheduled
on X86.

### Pipeline dump confirmation

```
$ llc -mtriple=x86_64-unknown-linux-gnu -O2 -debug-pass=Structure < /dev/null 2>&1 \
    | grep -iE 'pipeliner|hardware ?loop'
(no output)
```

vs. 219 total lines of pipeline output for X86 -O2, with `loop-reduce`, `machine-scheduler`,
etc. all present, so the grep is sound.

## Implication for bug hunting on default x86 -O2

Neither "Pipeliner drops MMO on iterated load" nor "HardwareLoops mishandles atomic ordering"
can be triggered through `llc -O2` with the default X86 triple. Reproducing either would
require an explicit non-x86 triple (e.g. `armv8a-none-eabi`, `aarch64`, `powerpc64le`,
`riscv64`, `hexagon`) and is therefore out of scope per the task constraint
("If not default-x86 -O2, write down briefly and move on. NO source-only.").
