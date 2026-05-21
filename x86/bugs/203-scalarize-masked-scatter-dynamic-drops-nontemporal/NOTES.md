# 203 — ScalarizeMaskedMemIntrin `scalarizeMaskedScatter` dynamic-mask path drops `!nontemporal` (and AAMD) on per-lane stores

Component: `llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp` line ~692

Sister bug to #202 (gather). The per-lane `StoreInst` created in the dynamic-mask loop never receives `Store->copyMetadata(*CI)`. `!nontemporal` lost → backend emits cached stores (`MOV`) instead of streaming stores (`MOVNTI`/`MOVNTDQ`).

## Severity

Default x86 -O2. Observable codegen regression.

## Fix

After the per-lane `Builder.CreateAlignedStore(...)` call, add `Store->copyMetadata(*CI);`.
