# X86FixupInstTuning: ProcessShiftLeftToAdd mutates MI but returns false

## File / lines
`llvm/lib/Target/X86/X86FixupInstTuning.cpp`, lines 295-308 (the
`ProcessShiftLeftToAdd` lambda) — and line 307 in particular:
```cpp
auto ProcessShiftLeftToAdd = [&](unsigned AddOpc) -> bool {
  if (MI.getOperand(NumOperands - 1).getImm() != 1)
    return false;
  if (!NewOpcPreferable(AddOpc, /*ReplaceInTie*/ true))
    return false;
  LLVM_DEBUG(dbgs() << "Replacing: " << MI);
  {
    MI.setDesc(TII->get(AddOpc));
    MI.removeOperand(NumOperands - 1);
    MI.addOperand(MI.getOperand(NumOperands - 2));
  }
  LLVM_DEBUG(dbgs() << "     With: " << MI);
  return false;     // <-- bug: should be `return true;`
};
```

## Reasoning
Every other `Process...` lambda in this file returns `true` after it has
mutated `MI`. `ProcessShiftLeftToAdd` mutates `MI` (changes opcode from
`PSLLWri`/`VPSLLDri`/... to the matching `PADD*` and rewrites the operand list)
but then returns `false`. The caller, `processInstruction`, returns this `false`
all the way up to the runner, so `Changed` is never set to `true` and
`NumInstChanges` is not incremented. The driver bails on later analyses
preservation: `runOnMachineFunction` returns `false` and the pass tells the
pipeline that *nothing changed*, even though the MachineFunction was mutated.

This is not a wrong-code bug for the shift itself (the rewritten instruction is
still PADD), but the lying-changed contract is dangerous: it inhibits pipeline
invalidation, breaks `STATISTIC` tracking, and causes any pass-driver dependency
that checks "did fixup-inst-tuning mutate?" to mis-schedule. With NPM and
`PreservedAnalyses::all()` returned for a function where MIR actually changed,
downstream analyses that cache MI-keyed data become stale.

## Candidate MIR
```mir
# llc -mtriple=x86_64-- -mattr=+sse2 -run-pass=x86-fixup-inst-tuning
name: f
body:
  bb.0:
    $xmm0 = PSLLWri $xmm0, 1
    RET 0, $xmm0
```

## Wrong outcome
After the pass, dump shows `PADDWrr $xmm0, $xmm0, $xmm0` (mutation occurred),
yet the legacy pass returns `false` (`bool Changed = ... || (false)`) so
`NumInstChanges` stays 0 and the NPM run() returns `PreservedAnalyses::all()`,
falsely claiming the IR was untouched. Stale machine analyses can then produce
incorrect downstream codegen.
