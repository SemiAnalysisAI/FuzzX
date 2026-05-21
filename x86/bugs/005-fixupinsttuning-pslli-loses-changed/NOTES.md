# 005 — X86FixupInstTuning `ProcessShiftLeftToAdd` mutates MI but returns false

Component: X86FixupInstTuning

## Source

`llvm/lib/Target/X86/X86FixupInstTuning.cpp`, around lines 295–308:

```cpp
auto ProcessShiftLeftToAdd = [&](unsigned AddOpc) -> bool {
  if (MI.getOperand(NumOperands - 1).getImm() != 1)
    return false;
  if (!NewOpcPreferable(AddOpc, /*ReplaceInTie*/ true))
    return false;
  LLVM_DEBUG(dbgs() << "Replacing: " << MI);
  {
    MI.setDesc(TII->get(AddOpc));            // <-- MUTATES MI
    MI.removeOperand(NumOperands - 1);       // <-- MUTATES MI
    MI.addOperand(MI.getOperand(NumOperands - 2));
  }
  LLVM_DEBUG(dbgs() << "     With: " << MI);
  return false;                              // <-- BUG: should be `true`
};
```

Every other `Process…` lambda in this file returns `true` after a mutation.
This one mutates `MI` (rewrites `PSLLWri 1` → `PADDWrr`, etc.) then returns
`false`, propagated through `processInstruction` up to `runOnMachineFunction`
unchanged. The pass therefore tells the pipeline that nothing changed:

- The `NumInstChanges` statistic stays at zero.
- The legacy PM `Changed` accumulator stays `false`.
- The NPM `run()` returns `PreservedAnalyses::all()`, so any subsequent
  analysis that depended on the old MIR opcode (per-MI metadata, scheduling
  cache, instruction-count summaries, machine-trace metrics) is not
  invalidated — stale data feeds the rest of the pipeline.

## Demonstration

`./cmd.sh` dumps MIR before and after the pass:

```
===== MIR going into X86FixupInstTuning (PSLLWri) =====
name:            shl_by_1
    renamable $xmm0 = PSLLWri killed renamable $xmm0, 1
===== MIR after X86FixupInstTuning (PADDWrr — mutation confirmed) =====
name:            shl_by_1
    renamable $xmm0 = PADDWrr killed renamable $xmm0, killed renamable $xmm0
```

The opcode changed, the operand list shrunk by one and grew by a copy of the
remaining source operand — yet the lambda returned `false`.

## Fix

Change the trailing `return false;` to `return true;`.

## Severity

Latent. Today's pipeline tolerates the lie because the immediately-following
register allocator / emitter inspects the MIR directly, but any cached
analysis (e.g., a MachineTraceMetrics derivative or a custom downstream pass)
that consults the "did this pass run?" signal will read stale data.
