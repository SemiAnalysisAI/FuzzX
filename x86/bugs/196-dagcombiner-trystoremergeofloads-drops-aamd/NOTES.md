# 196 — DAGCombiner `tryStoreMergeOfLoads` drops `!tbaa`/`!alias.scope`/`!noalias` on merged load+store

Component: `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp` lines ~23590-23625 (`tryStoreMergeOfLoads`)

When DAGCombiner merges adjacent `load i32 + store i32` pairs into one wide `load i64 + store i64`, it builds the new MMOs from raw flags only, never copying AAInfo. Two narrow loads + two narrow stores annotated with `!tbaa`/`!alias.scope`/`!noalias` collapse to one fully unannotated wide load + wide store. Downstream MachineAA queries lose the per-access scope and tag, which can re-enable accesses the original IR explicitly forbade.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel repro.ll -o -`

The MIR shows `MOV64rm` / `MOV64mr` with bare `(load (s64) from %ir.p01)` / `(store (s64) into %ir.d02)` — no AAInfo at all.

## Severity

Real correctness regression: downstream MIR-level AA loses scope. Fires in default x86 -O2 unconditionally for the merge pattern.

## Fix

Compute merged AAInfo via `MachineMemOperand` intersection in `tryStoreMergeOfLoads` and pass it through both `getLoad` and `getStore` calls.
