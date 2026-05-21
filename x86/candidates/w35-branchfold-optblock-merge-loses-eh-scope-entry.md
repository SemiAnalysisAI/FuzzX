# BranchFolding OptimizeBlock merges into PrevBB, drops MBB's EH-scope/funclet entry flag

File: llvm/lib/CodeGen/BranchFolding.cpp, function `BranchFolder::OptimizeBlock`, lines ~1450-1476.

## Pattern

The MBB-into-PrevBB splice transform is guarded by:

```cpp
if (... PrevBB.succ_size() == 1 && PrevBB.isSuccessor(MBB) &&
    !MBB->hasAddressTaken() && !MBB->isEHPad()) {
  ...
  PrevBB.splice(PrevBB.end(), MBB, MBB->begin(), MBB->end());
  PrevBB.removeSuccessor(PrevBB.succ_begin());
  PrevBB.transferSuccessors(MBB);
  ...
}
```

The check only excludes `MBB->isEHPad()`. `MachineBasicBlock` has separate flags
`IsEHScopeEntry` and `IsEHFuncletEntry` (`MachineBasicBlock.h:684, 697`) which
are NOT subsets of `IsEHPad`. A block can be an EH-scope entry (the first block
of a try region under WinEH) without being itself a pad.

When the block is spliced into PrevBB, the scope/funclet-entry markers stay on
the now-empty MBB (which is then removed elsewhere), and PrevBB gains the body
of an EH-scope entry without being marked as such. Downstream
`getEHScopeMembership` / FuncletLayout / WinEHPrepare consumers expect the
scope entry to be the canonical block that starts the scope; splicing the body
out can fragment the per-scope MBB set.

## Why it matters on x86

For Windows MSVC EH on x86 (and x86_64 SEH/CXX), PrevBB now contains code that
"belongs to" a child scope but is in the parent's MBB. FuncletLayout (which
groups blocks by EH scope) can place the resulting block in the wrong cluster,
breaking unwind/throw across this region.

## Suggested check

```cpp
!MBB->isEHPad() && !MBB->isEHScopeEntry() && !MBB->isEHFuncletEntry()
```

## Confidence

Source-level only; no runtime repro attempted. The transform may be unreachable
in practice if `IsEHScopeEntry` always implies `IsEHPad` (no evidence of that
in MachineBasicBlock.cpp), or if these blocks always have `pred_size() > 1`.
Worth a verification cycle with a SEH/CXX-EH triple.
