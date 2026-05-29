# 197 — DAGCombiner `mergeTruncStores` drops `!nontemporal` (MONonTemporal) and `!tbaa` (AAInfo)

Component: `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp` lines ~9929-9931 (`mergeTruncStores`)

When 4 byte stores of `trunc(lshr v, k)` get merged into a single i32 store, the 4-arg `getStore` overload is used. This overload drops MMOFlags (loses MONonTemporal) AND AAInfo (loses TBAA/alias.scope/noalias). The 4 stores all annotated with `!nontemporal !0, !tbaa !1` collapse to `MOV32mr ... (store (s32) into %ir.p01, align 1)` — no NT, no TBAA.

x86 visibility: the original would lower to MOVNTI; the merged version uses a plain MOV. Hardware non-temporal hint silently lost.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel repro.ll -o -`

## Severity

Default x86 -O2. NT hint is the whole point of the metadata; dropping it on merge is correctness for cache semantics.

## Fix

Use the 6-arg `getStore` overload that forwards MMOFlags + AAInfo in `mergeTruncStores`.
