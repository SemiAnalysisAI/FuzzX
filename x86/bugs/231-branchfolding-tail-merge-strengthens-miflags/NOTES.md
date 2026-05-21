# 231 — BranchFolding tail-merge STRENGTHENS MIflags (unsound direction): `nuw add` merged with plain `add` produces `nuw add`

Component: `llvm/lib/CodeGen/BranchFolding.cpp` `mergeCommonTails` lines ~838-873, root cause in `MachineInstr::isIdenticalTo` (`MachineInstr.cpp:673-765`) which doesn't compare `getFlags()`.

When two tail blocks contain `add nuw` and plain `add` with the same operands, `isIdenticalTo` returns true (flags ignored). `mergeCommonTails` then drops one MI and keeps the other, and the surviving MI is whichever happened to be in the kept block. If that's the `nuw add`, the merged code propagates `nuw` onto a control flow path where the original was plain — **strengthening** the asserted invariant on a path that may legitimately overflow.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -print-after=branch-folder repro.ll -o -`

Pre-merge: `nuw ADD32rr` in one block, plain `ADD32rr` in the other. Post-merge: single `nuw ADD32rr` (or plain — depending on which block survives). The result strengthens the invariant on the formerly-plain path.

Downstream consumers (e.g. `GISelValueTracking.cpp:338-339`) trust the flag and may emit incorrect code.

## Severity

Default x86 -O2. Soundness regression: strengthening a poison-generating flag in a path that didn't have it can introduce poison and downstream UB.

## Fix

Extend `isIdenticalTo` to compare `getFlags()` (or intersect flags in `mergeCommonTails`).
