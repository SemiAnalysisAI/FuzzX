# 199 — DAGCombiner `CombineConsecutiveLoads` drops MOInvariant + MONonTemporal + AAInfo

Component: `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp` lines ~17581-17582

When `BUILD_PAIR(load(p), load(p+sz))` is fused into one wide load, the 6-arg `getLoad` overload is used but neither MOInvariant nor MONonTemporal flags are forwarded, and AAInfo is left empty. Two i32 loads each tagged `!nontemporal`, `!invariant.load`, `!tbaa` collapse to one `MOV64rm` with bare `(load (s64) from %ir.p, align 4)` — no invariant, no NT, no TBAA.

The loss of MOInvariant is especially damaging: it disables hoisting/CSE of immutable loads after the combine, and re-introduces store-aliasing for what the user proved was immutable.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel repro.ll -o -`

## Severity

Default x86 -O2. Affects basic 32+32 → 64 vec/pair combine which fires very often.

## Fix

Forward MMOFlags (MOInvariant | MONonTemporal) and AAInfo from both source MMOs to the merged load.
