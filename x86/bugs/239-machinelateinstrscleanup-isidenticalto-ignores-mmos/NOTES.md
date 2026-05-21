# 239 — MachineLateInstrsCleanup `hasIdentical` ignores MMOs; merged invariant-load drops `!nontemporal`

Component: `llvm/lib/CodeGen/MachineLateInstrsCleanup.cpp` lines ~45-49 (`Reg2MIMap::hasIdentical`)

`hasIdentical` calls `MachineInstr::isIdenticalTo`, which does NOT compare `MachineMemOperand`s. Two invariant constant-pool loads of the same address — one plain, one `!nontemporal` — get merged with the survivor losing the NT hint.

Structurally identical to BranchFolder bugs (#226/#238) but in a different pass.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=machine-late-instrs-cleanup repro.ll -o -`

Pre-cleanup: T-path `MOVDQArm ... :: (invariant load (s128) ...)`; F-path `MOVDQArm ... :: (non-temporal invariant load (s128) ...)`. Post-cleanup: single survivor with `(invariant load (s128))` — NT silently lost.

## Severity

Default x86 -O2. NT hint corrupted on shared loads from constant pools.

## Fix

In `hasIdentical`, also require `hasIdenticalMMOs(*A, *B)` (or restrict the cleanup to MIs with no MMOs).
