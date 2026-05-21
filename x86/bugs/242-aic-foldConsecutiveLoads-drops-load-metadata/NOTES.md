# 242 — AggressiveInstCombine `foldConsecutiveLoads` drops non-AA metadata on merged wide load

Component: `llvm/lib/Transforms/AggressiveInstCombine/AggressiveInstCombine.cpp` lines ~1438-1444

The merged wide `LoadInst` is built fresh with only `LOps.AATags` re-applied. There is no `combineMetadataForCSE`/`combineMetadata` call. When both narrow loads carry `!nontemporal`, `!invariant.load`, `!noundef`, all are silently dropped on the merged load.

## Reproducer

`opt -passes=aggressive-instcombine -S repro.ll`

Two i32 loads, each with `!nontemporal`, `!invariant.load`, `!noundef`, merge into a single i64 load with NO metadata.

## Severity

Default x86 -O2 (aggressive-instcombine runs in default O2). NT/invariant hints silently lost on merged consecutive loads.

## Fix

After constructing the new merged `LoadInst`, call `combineMetadataForCSE(NewLI, LI1, /*DoesKMove=*/true)` and `combineMetadataForCSE(NewLI, LI2, /*DoesKMove=*/true)`.
