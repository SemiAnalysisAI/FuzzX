# MachineSink: dead physical-register def can still be live within source block

File: `llvm/lib/CodeGen/MachineSink.cpp:1881-1890` (`SinkInstruction`)
       and `llvm/lib/CodeGen/MachineSink.cpp:1432-1442` (`FindSuccToSinkTo`)

## Reasoning

`SinkInstruction` guards against sinking an instruction whose dead-marked
physreg def is live into the successor:

```cpp
// If the instruction to move defines a dead physical register which is live
// when leaving the basic block, don't move it because it could turn into a
// "zombie" define of that preg. E.g., EFLAGS.
for (const MachineOperand &MO : MI.all_defs()) {
  Register Reg = MO.getReg();
  if (Reg == 0 || !Reg.isPhysical())
    continue;
  if (SuccToSinkTo->isLiveIn(Reg))
    return false;
}
```

The guard only checks `SuccToSinkTo->isLiveIn(Reg)`. It does *not* check
whether `Reg` is read anywhere in the source block *between MI and the end
of the block*. If MI has `implicit-def dead $eflags` (e.g. a TEST-like
instruction marked dead because no follower reads it *at the time*),
**and** a later transformation (or a preceding cycle of MachineSink itself
in the same pass) creates a read of EFLAGS between MI and end-of-block
without re-running liveness analysis, sinking MI past that reader will hide
a real flag dependency.

Concretely: `FindSuccToSinkTo` accepts the sink if `MO.isDead()` is set for
the physreg def (line 1439-1442). The combination of (1) trusting the stale
`dead` flag on MI and (2) only checking liveness at `SuccToSinkTo`'s
livein-list means a use in the **tail of the source block** after MI is
invisible to the sink decision.

## When this matters

- After a previous pass cleared an implicit-def's `dead` flag on a different
  MI but didn't re-run MachineSink's liveness recomputation.
- For `EFLAGS` and `MXCSR`-style sticky physregs that the dead-flag on a
  TEST/CMP may not accurately represent post-coalescing.

## MIR repro shape

```
bb.0:
  successors: %bb.1
  %1:gr32 = ADD32rr %0, %0, implicit-def dead $eflags     ; the candidate
  JCC_1 %bb.2, 4, implicit $eflags                         ; reads EFLAGS from a prior CMP
  JMP_1 %bb.1
bb.1:
  ...uses %1...
```

If the EFLAGS used by `JCC_1` came from a CMP earlier in bb.0 and the ADD's
`dead $eflags` is honored, sinking the ADD past JCC_1 into bb.1 will leave the
JCC reading EFLAGS that ADD redefined-then-eliminated. Whether this is a true
miscompile depends on subsequent passes' treatment of the `dead` flag — but
the guard at line 1881-1890 alone is insufficient to prevent it; correctness
relies entirely on the upstream `dead` flag being accurate.

## Expected wrong outcome

Branch direction inversion when a CMP/JCC pair separated by a dead-EFLAGS-def
ADD gets the ADD sunk past the JCC. With `llc -O2 -run-pass=machine-sink`,
a manually constructed MIR with the layout above will (under the right
register pressure) actually move the ADD into bb.1, leaving `JCC_1` reading
the wrong EFLAGS.
