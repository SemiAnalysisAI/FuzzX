# 237 — MachineCSE drops one MMO's range/AAInfo on erase

Component: `llvm/lib/CodeGen/MachineCSE.cpp` (verified by w590)

When two MachineInstr loads with different `!range` MMOs are CSE'd, the surviving MI keeps only its own MMO; the eliminated MI's MMO (carrying different range information) is dropped. The surviving load thus claims a narrower scope than the union demands.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=machine-cse repro.ll -o -` shows the surviving `MOV32rm` carrying only one `!range`, with the other MMO's range information silently lost.

## Severity

Default x86 -O2. Downstream MachineSink/MachineLICM/scheduler decisions become wrong (range/AAInfo narrower than the union).

## Fix

Use `cloneMergedMemRefs` to combine MMOs from both MIs before erasing the duplicate, mirroring BranchFolder.
