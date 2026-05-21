# w231: MIMetadata(MachineInstr) ctor drops MMRA metadata, lost by every X86 pass that rebuilds an MI via BuildMI(MIMD)

## Summary

The `MIMetadata` constructor from a `MachineInstr` copies `DebugLoc`, `PCSections`, and `DeactivationSymbol` — but NOT `MMRAMetadata` (memory-model-relaxation-annotations). Any X86 codegen pass that mutates an instruction by re-`BuildMI`-ing a new one with `MIMetadata(*OldMI)` silently drops the MMRA metadata from the original MI.

This affects (at minimum):

- `X86FixupBWInsts.cpp` `tryReplaceLoad` (lines 296-297), `tryReplaceCopy` (lines 340-341), `tryReplaceExtend` (lines 368-369)
- `X86DomainReassignment.cpp` `InstrReplacer::convertInstr` (line 148), `InstrReplaceWithCopy::convertInstr` (line 260) — these use `MI->getDebugLoc()` only, no MIMetadata, so they also drop MMRA but separately
- and any other LLVM target/codegen pass that does `BuildMI(MF, MIMetadata(*MI), TII->get(NewOpc), ...)`

The result is a memory load/store loses its MMRA — used to reason about memory model relaxation between threads — when an unrelated optimization (e.g. BWInsts promoting `mov8` to `movzx8-to-32`) rewrites it.

## Source location

File: `llvm/include/llvm/CodeGen/MachineInstrBuilder.h`

```c++
133 class MIMetadata {
...
147   explicit MIMetadata(const MachineInstr &From)
148       : DL(From.getDebugLoc()), PCSections(From.getPCSections()),
149         DeactivationSymbol(From.getDeactivationSymbol()) {}
                                                              // <<< missing: MMRA(From.getMMRAMetadata())
...
159   MDNode *MMRA = nullptr;
```

Compare with the `MachineInstrBuilder::copyMIMetadata` at line 424 — it DOES propagate MMRA if MIMD has it:

```c++
424   const MachineInstrBuilder &copyMIMetadata(const MIMetadata &MIMD) const {
425     if (MIMD.getPCSections())
426       MI->setPCSections(*MF, MIMD.getPCSections());
427     if (MIMD.getMMRAMetadata())                      // checked, but MIMD.MMRA was never set
428       MI->setMMRAMetadata(*MF, MIMD.getMMRAMetadata());
429     if (MIMD.getDeactivationSymbol())
430       MI->setDeactivationSymbol(*MF, MIMD.getDeactivationSymbol());
431     return *this;
432   }
```

so the propagation path *exists* but is bypassed because the ctor never initializes the field.

## Reproducer

```ll
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(ptr %p) {
  %v = load i8, ptr %p, align 1, !mmra !0
  %r = zext i8 %v to i32
  ret i32 %r
}

!0 = !{!1, !2}
!1 = !{!"sync-as", !"thread"}
!2 = !{!"sync-as", !"foo"}
```

With `llc -mtriple=x86_64-unknown-linux-gnu -O2`, the IR's MOV8rm carries MMRA, and X86FixupBWInsts rewrites it to MOVZX32rm8. Pre-FixupBWInsts the MIR has the MMRA; post-FixupBWInsts the MMRA is gone:

```
# Pre  : renamable $al = MOV8rm renamable $rdi, 1, $noreg, 0, $noreg, mmra !{!1, !2}
# Post : renamable $eax = MOVZX32rm8 renamable $rdi, 1, $noreg, 0, $noreg  ; MMRA dropped
```

(The empirically-observable text-asm difference is none — MMRA does not currently affect x86 asm emission for non-atomic loads. But the metadata is intended to influence later memory-model-aware passes; dropping it is a correctness bug for any analysis that reads MMRA on the post-FixupBWInsts MI.)

## Impact

- Memory-model-aware passes that run AFTER X86FixupBWInsts (any MIR-level scheduler, machine licm, machine sink, store-forwarding, etc.) see a load/store with NO MMRA where the source IR explicitly attached one. That can:
  - allow speculative hoisting across a sync-as boundary that MMRA was meant to forbid;
  - allow reordering past atomics that share a sync-as group;
  - permit code motion that the !mmra annotation was inserted to prevent.
- The same drop happens in every pass that uses `BuildMI(MF, MIMetadata(*OldMI), ...)`: X86DomainReassignment, X86FixupBWInsts, plus any backend (AArch64, RISC-V, AMDGPU) that uses this idiom.

## Fix

```diff
-  explicit MIMetadata(const MachineInstr &From)
-      : DL(From.getDebugLoc()), PCSections(From.getPCSections()),
-        DeactivationSymbol(From.getDeactivationSymbol()) {}
+  explicit MIMetadata(const MachineInstr &From)
+      : DL(From.getDebugLoc()), PCSections(From.getPCSections()),
+        MMRA(From.getMMRAMetadata()),
+        DeactivationSymbol(From.getDeactivationSymbol()) {}
```

at line 147. The `copyMIMetadata` consumer is already correct.

## Why this is a backend bug, not just CodeGen

While the constructor lives in `include/llvm/CodeGen`, the resulting silent loss is observable through every X86 machine pass that rebuilds an MI from a previous one. Because the X86 backend's `FixupBWInsts` is the most common pass that promotes byte/word loads to dword loads using this idiom, and it runs unconditionally at -O1/O2, the bug surfaces on every x86 program that has !mmra on a byte or word load.
