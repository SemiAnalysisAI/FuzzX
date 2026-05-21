# w346: LiveRangeEdit::eliminateDeadDef ReadsPhysRegs branch strips non-physreg ops then drops MMOs — drops FrameIndex / GlobalAddress operands rather than preserving liveness shape

## Status
SUSPECTED. The branch is reachable for any deletion candidate that reads an unreserved physreg. KILL conversion is intentionally lossy, but the lossiness combined with `dropMemRefs` can lose otherwise-useful liveness info that downstream passes still consult.

## Source
`llvm/lib/CodeGen/LiveRangeEdit.cpp:328-338`
```cpp
else if (ReadsPhysRegs) {
  MI->setDesc(TII.get(TargetOpcode::KILL));
  // Remove all operands that aren't physregs.
  for (unsigned i = MI->getNumOperands(); i; --i) {
    const MachineOperand &MO = MI->getOperand(i-1);
    if (MO.isReg() && MO.getReg().isPhysical())
      continue;
    MI->removeOperand(i-1);
  }
  MI->dropMemRefs(*MI->getMF());
  LLVM_DEBUG(dbgs() << "Converted physregs to:\t" << *MI);
}
```

## Description
When a dead def is eliminated and `ReadsPhysRegs` is true (the instruction reads some unreserved physical register so we cannot DCE it cleanly), the code converts the instruction to `KILL` and removes all non-physreg operands. The intent is to leave only physreg uses so register liveness is preserved.

But the loop walks ALL operands and removes anything that is not `MO.isReg() && MO.getReg().isPhysical()`. That includes:
- FrameIndex operands (`%stack.N`)
- GlobalAddress operands (`@g`)
- ExternalSymbol operands (`@__some_helper`)
- Immediates (harmless for KILL)
- RegMasks (call-clobber masks!)

If the original instruction was, say, an inline-loaded global address with an attached symbol used by speculative-load-hardening or a stack-spill pseudo, dropping the FrameIndex / GlobalAddress operand AND `dropMemRefs` simultaneously loses ALL information about which memory the original instruction touched. Subsequent passes that inspect `MachineFrameInfo` use counts or symbol tracking may end up with a stale "this slot is referenced by 0 instructions" view.

The RegMask case is the most concerning: KILL with RegMask removed means later analyses no longer know about clobber semantics — but in practice an instruction that both reads physregs and has a regmask is a call, which is not removable here. Less hypothetical: physreg-defining instruction with a tied FrameIndex use.

The original instruction is preserved as-is in the `isOrigDef` branch (line 300-320). Only when we reach `ReadsPhysRegs` do we sledgehammer. The asymmetry suggests this path was added for an edge case (PIC base, RIP-relative loads) without a careful audit of what FrameIndex / GlobalAddress operands look like on a KILL.

## Observed
Hard to trigger from -O2 IR because the `ReadsPhysRegs` path requires a dead-def instruction reading an unreserved physreg that survives until the spiller's DCE — typically a side-effect-free instruction whose physreg use is incidental. Most common path is X86 `LEA32r $eflags` reads or constant-pool loads through `$rip`.

A small targeted test (rip-relative constant pool reload that gets DCE'd by the spiller):
```
@g = global i64 1
define i64 @test(i64 %x) {
  %p = ptrtoint ptr @g to i64
  %d = sub i64 %p, %p          ; dead
  ret i64 %x
}
```
Inspecting `-debug-only=regalloc` traffic confirms the path is hit on certain large fixtures.

## Severity
Likely silent bug: the resulting KILL is still semantically a KILL, so liveness is fine. The risk is downstream passes that walk the function looking for FrameIndex references for a specific stack slot will MISCOUNT — the dead-def's FrameIndex was the only "user" of that slot prior to elimination, but post-elimination the slot still exists in `MachineFrameInfo` with zero apparent references.

## Fix sketch
Either:
1. Stop converting to KILL for instructions that read unreserved physregs AND have non-physreg operands besides simple immediates. Instead, keep the instruction and mark the dead defs as `<dead>`.
2. Before dropping FrameIndex / GlobalAddress operands, notify `MachineFrameInfo::RemoveStackObject` style bookkeeping.
3. Move the `dropMemRefs` call BEFORE the operand strip so that the strip is purely about register flags, OR keep MMOs and rely on the KILL opcode to ignore them.
