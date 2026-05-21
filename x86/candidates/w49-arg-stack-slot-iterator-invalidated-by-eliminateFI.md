# X86ArgumentStackSlotRebase: `for (MachineOperand &MO : MI.operands())` is invalidated by `TRI->eliminateFrameIndex`

## File
`llvm/lib/Target/X86/X86ArgumentStackSlotRebase.cpp`, lines 169-190.

## Code

```cpp
for (MachineBasicBlock &MBB : MF) {
  for (MachineInstr &MI : MBB) {
    int I = 0;
    for (MachineOperand &MO : MI.operands()) {     // <-- iteration over MI operands
      if (MO.isFI()) {
        int Idx = MO.getIndex();
        if (!MFI.isFixedObjectIndex(Idx))
          continue;
        int64_t Offset = MFI.getObjectOffset(Idx);
        if (Offset < 0)
          continue;
        if (MI.isDebugInstr())
          continue;
        // Replace frame register with argument base pointer and its offset.
        TRI->eliminateFrameIndex(MI.getIterator(), I, ArgBaseReg, Offset);
        Changed = true;
      }
      ++I;
    }
  }
}
```

## Bug

`TargetRegisterInfo::eliminateFrameIndex(MI, FIOperandNum, BaseReg, Offset)` is documented to (and X86 does) replace operand `FIOperandNum` (the FI) with a register, may shift other operands, and may even *insert additional MIs* before or after `MI` to materialize complex addressing. The current operand iterator `MO` (and the `range_iterator` driving the `for` loop) refers to storage inside `MI`'s operand vector. After `eliminateFrameIndex`:

- The operand at index `I` is rewritten to a `Register` operand.
- The operand at index `I+1` (was `AddrScaleAmt`) may be rewritten as the immediate `Offset`.
- The operand at index `I+2`/`I+3` (Index/Disp) may be cleared.

These rewrites do **not** add or remove operands in the typical x86 case (eliminate replaces an FI memory-operand block in-place). However, the `range_iterator` for `MachineInstr::operands()` is a thin wrapper over `mop_iterator`, which is a pointer into the operand storage. If the operand vector ever reallocates (e.g., when `eliminateFrameIndex` calls `BuildMI` and that path causes operand-slab reallocation indirectly), the iterator is invalidated.

More importantly, **`++I` advances past the rewritten FI operand**, but the rewritten operand at index `I` is now a register, not an FI. The subsequent `++I` iteration steps to the next operand and runs the `MO.isFI()` check against it — which may also be an FI if the instruction has multiple memory operands (e.g., a tail-call `TCRETURNmi64` has two memory address operands). The replacement of the *second* FI then happens with a stale `I` value that doesn't account for any operand shifts the *first* call may have made.

In practice on x86, `eliminateFrameIndex` rewrites operands in place (no shift), so `++I` lands correctly. But the pattern is fragile and the in-loop call to a function with such broad rewriting power is a footgun. Compare with the standard pattern in `PrologEpilogInserter::replaceFrameIndices`:

```cpp
for (unsigned i = 0; i < MI.getNumOperands(); ) {
  if (!MI.getOperand(i).isFI()) { ++i; continue; }
  TRI->eliminateFrameIndex(MI, i, ...);
  // Re-scan from i (operand was rewritten); do NOT advance.
}
```

which re-reads the operand count each iteration and does not hold a `MachineOperand &` across the rewrite.

## Why it matters

If a future change in `X86RegisterInfo::eliminateFrameIndex` ever needs to *remove* the FI operand instead of rewriting it in place (e.g., to switch to a 2-op LEA encoding), this loop will silently corrupt because `MO` becomes dangling and `++I` over-advances. This is a maintenance/landmine concern, not a present miscompile.

## Confidence

Low (latent; no current miscompile). Suggest `for (unsigned I = 0, E = MI.getNumOperands(); I != E; )` index-based loop with re-read after rewrite.
