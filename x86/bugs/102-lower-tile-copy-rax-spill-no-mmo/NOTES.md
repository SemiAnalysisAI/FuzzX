# X86LowerTileCopy: RAX spill-slot fallback does not check whether RAX is the *defined* tile-copy operand context

## File
`llvm/lib/Target/X86/X86LowerTileCopy.cpp`, lines 89-156.

## Code

```cpp
for (MachineInstr &MI : llvm::make_early_inc_range(reverse(MBB))) {
  UsedRegs.stepBackward(MI);
  if (!MI.isCopy())
    continue;
  ...
  // Pick a killed register to avoid a save/reload.
  Register GR64Cand = X86::NoRegister;
  for (auto RegT : GR64Regs.set_bits()) {
    if (UsedRegs.available(RegT)) {
      GR64Cand = RegT;
      break;
    }
  }
  ...
  if (GR64Cand) {
    BuildMI(MBB, MI, DL, TII->get(X86::MOV64ri), GR64Cand).addImm(64);
  } else {
    ...
    // Spill RAX, then overwrite with the stride 64
    addFrameReference(BuildMI(MBB, MI, DL, TII->get(X86::MOV64mr)),
                      StrideSS)
        .addReg(X86::RAX);
    BuildMI(MBB, MI, DL, TII->get(X86::MOV64ri), X86::RAX).addImm(64);
  }
  // tilestored %tmm, (%sp, %idx)
  ...
  unsigned Opc = GET_EGPR_IF_ENABLED(X86::TILESTORED);
  MachineInstr *NewMI =
      addFrameReference(BuildMI(MBB, MI, DL, TII->get(Opc)), TileSS)
          .addReg(SrcReg, getKillRegState(SrcMO.isKill()));
  MachineOperand *MO = &NewMI->getOperand(X86::AddrIndexReg);
  MO->setReg(GR64Cand ? GR64Cand : X86::RAX);
  ...
```

## Bug candidate

The `UsedRegs.available(RegT)` scan iterates `GR64Regs.set_bits()`, which is constructed from `TRI->getAllocatableSet(MF, GR64RegClass)`. The first bit found is selected. The iteration order of `BitVector::set_bits()` is ascending bit index, so the picked register depends on the static GR64 register numbering. RAX is typically a low-numbered GR64 — meaning RAX is the **first** candidate the loop considers.

If RAX is live at MI (e.g., it carries a return value into a tail predecessor of MBB, or it's the SysV varargs vector-count register), `UsedRegs.available(X86::RAX)` returns false and the loop moves on to RBX, RCX, etc. Eventually, if *every* allocatable GR64 is live (a rare but legitimate post-allocator state), `GR64Cand` stays `X86::NoRegister` and we fall into the spill-RAX path.

In that fall-through, the code unconditionally uses `X86::RAX` for stride — **even though RAX was just determined to be live**. The spill-and-restore handles the live-value preservation, so this should be correct. But there is no protection against the case where `SrcReg` or `DstReg` of the TILE copy *is itself address-encoded as RAX* in some downstream instruction — see below.

The real concern: `GR64Regs` is filtered through `getAllocatableSet`, which excludes the **base pointer** when `hasBasePointer()` is true. So if RBX is the base pointer, it is *not* in `GR64Regs`. Now consider a function where the tile copy sits between two ADJCALLSTACK boundaries with every allocatable GR64 except RBX live across MI (e.g., complex Win64-X86_RegCall). The loop finds nothing, falls through to "spill RAX," and emits `MOV64mr` for RAX. But the `addFrameReference(..., StrideSS)` lowering uses the **stack frame** address mode, which on a function with a dynamically-aligned stack relies on the base pointer (RBX). At this point in compilation (pre-PEI per the pass header), the FI gets resolved later. That resolution will use RBX if `hasBasePointer()`. So the stack reference itself is fine.

What is *not* fine: the **spill-RAX** sequence assumes that immediately above the TILE copy, RAX holds a value that must be preserved through `tilestored` and `tileloadd`, then restored. If RAX is *also* an implicit operand of the TILE copy MI or any pseudo emitted between `MOV64ri RAX, 64` and `MOV64rm RAX` (post-RA can introduce CFI directives, EH_LABEL, etc., but those don't touch GPRs), the restore at line 154 would re-clobber the new value the optimizer wanted us to compute.

## Why it might be benign

`UsedRegs` is computed by `stepBackward` over the rest of MBB starting from `addLiveOuts`. At the point of MI, `UsedRegs` correctly tracks what's live ABOVE MI. So the spill correctly preserves RAX *as it exists above MI*. After the `tileloadd`, RAX is reloaded — restoring exactly what was above MI. So semantically the substitution is invisible to surrounding code.

## Real bug: no MachineMemOperand on spill/reload

The four MOVs to `StrideSS` and `TileSS` are built via `addFrameReference`, which generates the address operands but does *not* attach a `MachineMemOperand`. This means alias analysis in later passes sees these spills as unanalyzed memory ops and cannot reorder around them safely (conservative — not a miscompile, but a missed-opt).

The spill/reload of RAX is also missing FI tracking that the frame-info knows about beyond `CreateSpillStackObject`. Compare to e.g. `X86InstrInfo::storeRegToStackSlot`, which calls `MF.getMachineMemOperand(...)` with the right size/align/type. Without an MMO, post-RA scheduler may reorder around the spill, breaking the protocol that "RAX above = RAX below" for the user's value.

## Confidence

Low-medium for the missing-MMO concern (likely real, may produce wrong code under post-RA scheduling at high opt levels with AMX heavy workloads). The "no GR64 available" path itself looks semantically OK.
