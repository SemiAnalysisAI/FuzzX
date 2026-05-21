# 225 — LoopUnroll `loadCSE` drops `!nontemporal` (and `!align`) when CSE'ing two same-address loads

Component: `llvm/lib/Transforms/Scalar/LoopUnrollPass.cpp` `loadCSE` lines ~277-340 (esp. 316-319)

After full/partial unroll, two same-address loads in adjacent iterations are merged via plain `replaceAllUsesWith`, with NO `combineMetadataForCSE`. If the eliminated load carries `!nontemporal` (and the surviving leader doesn't), the hint is silently dropped — every iteration past the first loses NT.

## Reproducer

`opt -passes=loop-unroll -unroll-allow-partial -unroll-count=2 -S repro.ll`

After unroll, the merged surviving load `%l1` lacks `!nontemporal`; the `%l2` that carried it is gone. Backend emits plain MOV instead of MOVNTDQA.

## Severity

Default x86 -O2 (loop-unroll is in default O2). NT loads inside loops get silently downgraded after unroll.

## Fix

Replace bare RAUW with `combineMetadataForCSE(SurvivingLoad, EliminatedLoad, /*DoesKMove=*/false)`.
