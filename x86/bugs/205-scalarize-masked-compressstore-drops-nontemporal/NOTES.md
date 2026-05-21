# 205 — ScalarizeMaskedMemIntrin `scalarizeMaskedCompressStore` drops `!nontemporal` (and AAMD) on per-lane stores

Component: `llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp` lines ~877 (const-mask), ~927 (dyn-mask)

Mirror of #204 (expandload). Per-lane `StoreInst` never receives `copyMetadata(*CI)`.

## Severity

Default x86 -O2. NT hint silently lost on every compressstore.

## Fix

Add `Store->copyMetadata(*CI);` at both per-lane store sites.
