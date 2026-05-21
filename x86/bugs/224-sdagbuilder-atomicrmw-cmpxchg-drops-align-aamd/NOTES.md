# 224 — SDAGBuilder `visitAtomicRMW`/`visitAtomicCmpXchg` drop `I.getAlign()` and AAMD

Component: `llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp` lines 5213 (cmpxchg) and 5285 (atomicrmw)

Both helpers build the MMO with `DAG.getEVTAlign(MemVT)` (i.e., the natural alignment of the value type) instead of `I.getAlign()` (the explicit IR alignment). They also pass `AAMDNodes()` literal instead of `I.getAAMetadata()`. Sibling `visitAtomicLoad`/`visitAtomicStore` (5332/5369) use `I.getAlign()` correctly.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel repro.ll -o -`:

Input has `atomicrmw add ... align 32`. The resulting MIR `LXADD32` MMO reports `alignment: 1` (or natural `align 4`), losing the explicit `align 32`. Backend AA / sched / coalescer all consume `MMO->getAlign()` directly.

## Severity

Default x86 -O2. Loses explicit alignment guarantees on every atomicrmw/cmpxchg, defeating downstream optimizations that depend on alignment (load-combining, vectorization of subsequent loads).

## Fix

Use `I.getAlign()` (and `I.getAAMetadata()`) in `visitAtomicRMW` / `visitAtomicCmpXchg`, mirroring the load/store siblings.
