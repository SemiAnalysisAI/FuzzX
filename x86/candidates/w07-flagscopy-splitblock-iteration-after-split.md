# X86FlagsCopyLowering: JmpIs rewriting after splitBlock leaves stale LastJmpMBB tracking

File: llvm/lib/Target/X86/X86FlagsCopyLowering.cpp:694-704, 149-247

## Reasoning

After collecting branch instructions into `JmpIs`, the rewriter walks them and splits the containing block when more than one jmp is in the same block:

```cpp
MachineBasicBlock *LastJmpMBB = nullptr;
for (MachineInstr *JmpI : JmpIs) {
  if (JmpI->getParent() == LastJmpMBB)
    splitBlock(*JmpI->getParent(), *JmpI, *TII);
  else
    LastJmpMBB = JmpI->getParent();
  rewriteMI(*TestMBB, TestPos, TestLoc, *JmpI, CondRegs);
}
```

After `splitBlock`, `JmpI` and all subsequent JmpIs in the original block are spliced into the new `NewMBB` (`NewMBB.splice(NewMBB.end(), &MBB, SplitI.getIterator(), MBB.end())`). On the next iteration, `JmpI->getParent()` is now NewMBB which is NOT equal to `LastJmpMBB` (still the *pre-split* MBB), so the `else` branch runs and `LastJmpMBB` is updated to NewMBB. But that means *three* jumps originally in the same block are split into MBB → NewMBB (one jump) and then NewMBB still contains two more jumps; the loop sees the second one is in NewMBB (now LastJmpMBB) and on the third jump correctly splits again. The mechanics work for the splitting itself.

The subtle bug is in `rewriteMI` after splitting: `rewriteMI` calls `insertTest(*MI.getParent(), MI.getIterator(), ...)` which inserts a TEST8rr *before* the jmp in its current parent (NewMBB). However the CondReg captured at `TestPos` was originally in `TestMBB`; if `TestMBB` no longer dominates NewMBB after a split (it should still dominate since NewMBB is just a tail-split), this is OK. But the splitBlock copies successors based on `IsEdgeSplit` heuristics, and PHIs in successors are rewritten *only* for the moved tail. If the predecessor of NewMBB (the original MBB) still falls through via the JCC just before the split point — and that JCC was *itself* rewritten earlier in the same loop using the *original* MBB's iterator — the rewritten test was inserted into the original MBB *after* the split has stolen instructions from `SplitI` onward. `splitBlock` is called *between* iterations, so an earlier-rewritten JCC in MBB stays in MBB, but its predecessor TEST8rr is fine.

The real bug: `splitBlock` (line 175-184) computes `IsEdgeSplit` by looking at successor MBB references in the *moved* terminators. When the *first* of two JCCs in a block targets the same successor as a later JCC's fallthrough, `IsEdgeSplit=true` triggers an `MI.addOperand(MF, OpV); MI.addOperand(MF, MachineOperand::CreateMBB(&NewMBB))` (line 239-240). This *appends* a new PHI entry, but the existing entry for `&MBB` was NOT replaced (the `continue` at line 235 only fires for unsplit successors). So the resulting PHI has entries for both `MBB` and `NewMBB` pointing at the same `OpV` — but the value coming from NewMBB may be different (it has gone through the rewritten TEST/JCC sequence which may now branch in a different basic block) and is no longer reachable from `MBB` directly. The PHI ends up with stale incoming edges from a predecessor that no longer reaches the successor.

## Reproducer sketch

```
bb.0:
  %f:gr64 = COPY $eflags                ; capture
  $eflags = COPY %f
  JCC_1 %bb.2, 4, implicit $eflags      ; first jcc -> bb.2
  JCC_1 %bb.2, 5, implicit $eflags      ; second jcc -> same successor bb.2
  JMP_1 %bb.1

bb.1:
  ...
bb.2:
  %x:gr32 = PHI %y, %bb.0, %z, %bb.1
```

After lowering, the loop rewrites both JCCs; the second triggers splitBlock; the PHI in bb.2 is updated to have a duplicate (bb.0, %y) + (NewMBB, %y) entry, but the JCC in bb.0 no longer flows to bb.2 along the path where the PHI value should be `%y`.

## Expected wrong outcome

PHI in bb.2 has wrong predecessor list relative to actual CFG. Either verifier failure ("PHI operand is not present in predecessor list") or wrong runtime value selected. Reproduce with `llc -O2 -verify-machineinstrs` on a function that has multiple conditional jumps to the same successor through an EFLAGS COPY.
