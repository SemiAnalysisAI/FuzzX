# w232: X86FixupBWInsts tryReplaceLoad/Copy/Extend silently drops MachineInstr MIFlags

## Summary

The three replacement helpers in `X86FixupBWInsts.cpp` — `tryReplaceLoad`, `tryReplaceCopy`, and `tryReplaceExtend` — build a *new* `MachineInstr` via `BuildMI(*MF, MIMetadata(*MI), TII->get(NewOpc), NewDestReg)` and discard every `MachineInstr::MIFlag` on the original. Only the original's explicit operands and memrefs are propagated.

The flags that get dropped include:

- `FrameSetup` and `FrameDestroy` — CFI/PEI rely on them to emit correct call-frame information.
- `NoSWrap` / `NoUWrap` — though primarily used on arithmetic, may be present on loads in some pipelines.
- `IsUnpredictable` — used by branch-likely scheduling.
- `Unmergeable` — used to prevent merging by post-RA passes.
- `BundledPred` / `BundledSucc` — silently breaks bundles if the original MOV was bundled.

When a MOV8rm or MOV16rm appears at the start of an x86 function epilogue (frame-restore), it can carry the `FrameDestroy` flag. After `X86FixupBWInsts` rewrites it to `MOVZX32rm8/16`, that flag is gone — the new instruction is no longer recognized as part of the frame destroy sequence by any consumer of `MI.getFlag(FrameDestroy)`.

## Source location

`llvm/lib/Target/X86/X86FixupBWInsts.cpp` 

- `tryReplaceLoad` lines 285-314 — `BuildMI(*MF, MIMetadata(*MI), TII->get(New32BitOpcode), NewDestReg);` then copies operands 1..N and memrefs only.
- `tryReplaceCopy` lines 316-351 — `BuildMI(*MF, MIMetadata(*MI), TII->get(X86::MOV32rr), NewDestReg)` then copies one source and selected implicits.
- `tryReplaceExtend` lines 353-385 — same pattern.

None of the three call `MIB.setMIFlags(MI->getFlags())` or equivalent.

## Reproducer

```ll
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(ptr %p) "frame-pointer"="all" {
entry:
  %v = load i8, ptr %p, align 1
  %r = zext i8 %v to i32
  ret i32 %r
}
```

`llc -mtriple=x86_64-unknown-linux-gnu -O2 /tmp/x86bugs/test_bwinsts_frame_flags.ll -print-after=x86-fixup-bw-insts` will show the post-FixupBWInsts MIR has lost any `frame-destroy`/`frame-setup` flag that the original `MOV8rm` carried — even though FrameSetupOpcode-class instructions in the function should preserve their FrameSetup/FrameDestroy markers for CFI emission.

For a more focused observable bug, a `MOV8rm` that the X86 backend creates during epilogue lowering with the FrameDestroy flag, followed by FixupBWInsts converting it to a MOVZX32rm8, results in the new MOVZX32rm8 carrying no FrameDestroy flag — which mis-instructs `MachineModuleInfo` and CFI-emission code that inspect `MI.getFlag(MachineInstr::FrameDestroy)`.

## Why this matters

CFI emission, frame-aware scheduling, and frame-aware code motion all consume `MIFlag::FrameSetup` / `MIFlag::FrameDestroy`. Silently dropping them on a MOV-derived load is an information loss that may not (today) produce observable asm differences in trivial test cases, but it:

1. Breaks any later consumer reading these flags via `MI.getFlag(FrameDestroy)`.
2. Loses analysis info on `NoSWrap`/`NoUWrap` (for ND/NF variants on APX) that other passes rely on.
3. Inconsistent with what the wider codegen tradition does on instruction replacement.

## Fix

```diff
   MachineInstrBuilder MIB =
       BuildMI(*MF, MIMetadata(*MI), TII->get(New32BitOpcode), NewDestReg);
+  MIB.setMIFlags(MI->getFlags());
```

at lines 297, 341, 369. Three one-line additions, one per helper.

## Note on relation to w231

This bug shares root cause with w231 (`MIMetadata` ctor drops MMRA), but is independent: even after w231 is fixed, MIFlags are NOT carried by `MIMetadata` at all. The right fix here is an explicit `setMIFlags` call.
