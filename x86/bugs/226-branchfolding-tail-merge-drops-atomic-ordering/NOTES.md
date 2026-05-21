# 226 — BranchFolding tail-merge equates MMOs ignoring atomic ordering / syncscope (silently merges)

Component: `llvm/lib/CodeGen/BranchFolding.cpp` `mergeOperations` lines 789-836 (esp. line 822), plus root cause in `MachineMemOperand::operator==` (`MachineMemOperand.h:349-360`) which omits `getSuccessOrdering()`/`getFailureOrdering()`/`getSyncScopeID()`.

When two tail blocks contain a load-from-same-address pair where one is `load atomic monotonic` and the other is plain `load`, BranchFolder tail-merges them into a single load. `cloneMergedMemRefs` uses MMO `operator==` for dedup, which treats the two as identical and silently drops the monotonic ordering — the merged MMO is plain `(load (s32))`.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -print-before=branch-folder -print-after=branch-folder repro.ll -o -`

Pre-BranchFolder MIR has `(load monotonic (s32))` in one block and `(load (s32))` in the other. Post-BranchFolder: only `(load (s32))` survives. The atomic ordering is silently lost.

## Severity

Default x86 -O2. Atomic ordering is *not* a hint — it's part of the memory model contract; dropping it on the post-merge MMO can corrupt downstream MachineScheduler / MachineLICM decisions about reordering past synchronization points.

## Fix

Either bail in `mergeOperations` when MMOs disagree on `SuccessOrdering`/`FailureOrdering`/`SyncScopeID`, or extend `MachineMemOperand::operator==` to compare those fields.
