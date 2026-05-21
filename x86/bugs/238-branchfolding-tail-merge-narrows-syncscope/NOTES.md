# 238 — BranchFolding tail-merge silently narrows system-scope atomic to `syncscope("singlethread")`

Component: `llvm/lib/CodeGen/BranchFolding.cpp` mergeOperations (root cause in `MachineMemOperand::operator==` omitting `getSyncScopeID()`).

Sibling of #226. When tail-merging two stores where one is system-scope and the other is `syncscope("singlethread")`, the merged MMO arbitrarily picks one. If singlethread wins, the system-scope path is silently narrowed — its cross-thread ordering contract is no longer enforced.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -print-after=branch-folder repro.ll -o -`

Pre-merge: T-path `(store monotonic (s32))` (system) + F-path `(store syncscope("singlethread") monotonic (s32))`. Post-merge: single store inheriting one of the scopes — both paths now share `syncscope("singlethread")`, narrowing the originally-system path.

## Severity

Default x86 -O2. Cross-thread synchronization can be silently broken.

## Fix

Extend `MachineMemOperand::operator==` to compare `SyncScopeID` (or bail in `mergeOperations` when they disagree).
