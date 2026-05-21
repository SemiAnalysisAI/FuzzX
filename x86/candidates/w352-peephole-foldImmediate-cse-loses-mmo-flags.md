# w352: PeepholeOptimizer foldImmediate CSE arm drops MMO/flags from the eliminated identical move

## Severity
Latent miscompile / debug-info regression.

## Suspicious code
`llvm/lib/CodeGen/PeepholeOptimizer.cpp:1482-1492`:

```cpp
if (MRI->getVRegDef(Reg) &&
    MI.isIdenticalTo(*II->second, MachineInstr::IgnoreVRegDefs)) {
  Register DstReg = MI.getOperand(0).getReg();
  if (DstReg.isVirtual() &&
      MRI->getRegClass(DstReg) == MRI->getRegClass(Reg)) {
    MRI->replaceRegWith(DstReg, Reg);
    MRI->clearKillFlags(Reg);
    MI.eraseFromParent();
    Deleted = true;
  }
}
```

This is a CSE that fires when `TII->foldImmediate` returned true but the use-MI is still identical to the imm-def-MI after fold (i.e., it's essentially the same constant move). All uses of `DstReg` are redirected to `Reg`, and the redundant `MI` is erased.

`MachineInstr::isIdenticalTo` (with `IgnoreVRegDefs`) does **not** compare:
- `MachineMemOperand` / `AAInfo` / alias scopes
- `MIFlag`s (e.g., `NoFPExcept`, `NoSWrap`, `Reassoc`, ...)
- `getPCSections()` / `getMMRAMetadata()` (these are checked by `isIdenticalTo` for pre/post symbol but not for the metadata payload)
- `DebugLoc`

When `MI` is erased, the surviving `II->second` keeps only its own flags. If `MI` carried `noimplicitfloat`-style flags or PCSections metadata that the surviving copy doesn't, downstream passes (e.g., `MIRSampleProfileLoader`, `BasicBlockSections`) observe inconsistent state. More concretely, if `MI` has a non-trivial MMO (it can, since `foldImmediate` runs on arbitrary instructions including those that mayLoad / mayStore — though typically `MoveImmediate` instructions don't have MMOs), the merge silently drops it.

This is the same family of bug as w351 (`MachineInstr::isIdenticalTo` ignoring MMOs/flags) but in a different caller.

## Probe IR
Hard to manifest at the IR level because the X86 `MoveImmediate` instructions (`MOV32ri`, `MOV64ri`, ...) don't carry MMOs. The exposure is targets where `getConstValDefinedInReg` returns true for instructions that do carry side-channel state (e.g., debug intrinsics that survived to MIR), or future enhancements that broaden the set of "move-immediate-like" instructions to include ones with MMOs/flags.

## Root cause summary
The CSE arm trusts `isIdenticalTo` as a full-equivalence predicate, but it intentionally ignores MMOs and `MIFlag`s. When `MI` is erased, no merge of the dropped metadata is performed.

## Fix sketch
Before `MI.eraseFromParent()`, copy missing-but-present-on-MI metadata to `II->second`:
- `II->second->cloneMergedMemRefs(*MF, {II->second, &MI});`
- `II->second->setFlags(II->second->getFlags() & MI.getFlags());` (intersection — keep only flags both prove)
- merge `PCSections` / debug-loc with the standard `MachineInstr::mergeDebugLoc` helper.
