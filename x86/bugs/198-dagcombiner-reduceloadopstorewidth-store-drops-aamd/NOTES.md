# 198 — DAGCombiner `ReduceLoadOpStoreWidth` keeps load MMO but drops store MMO

Component: `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp` lines ~22441-22450

The shrink fold for `store(or(load,imm), p)` produces a narrower load+store. The load side correctly passes flags + AAInfo. The store side uses the 4-arg `getStore` overload that drops both MMOFlags (MONonTemporal) and AAInfo (TBAA).

Result is *asymmetric* MMO loss visible directly in the MIR — the resulting `OR8mi` has one MMO with `non-temporal, !tbaa !0` (load) and another with neither (store). The non-temporal STORE hint and the store-side TBAA tag silently disappear; the read-side keeps both.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel repro.ll -o -`

## Severity

Default x86 -O2. Asymmetric MMO is observably wrong: backend MIR-level AA can refuse to swap with the read but accept reorders past the write, which is the opposite of what the user encoded.

## Fix

Use the matching 6-arg `getStore` overload that forwards MMOFlags + AAInfo, like the load side does.
