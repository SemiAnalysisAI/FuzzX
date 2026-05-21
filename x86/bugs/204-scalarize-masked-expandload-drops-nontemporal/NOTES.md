# 204 — ScalarizeMaskedMemIntrin `scalarizeMaskedExpandLoad` drops `!nontemporal` (and AAMD) on BOTH constant- and dynamic-mask paths

Component: `llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp` lines ~748 (const-mask) and ~806 (dyn-mask)

Unlike `scalarizeMaskedLoad`, expandload has no all-true / splat short-cut, so EVERY use loses metadata. Per-lane `LoadInst`s created via `Builder.CreateAlignedLoad` never receive `copyMetadata(*CI)`.

Reproducer carries `!nontemporal !0` on the intrinsic; the lowered scalar loads have no metadata.

## Severity

Default x86 -O2. NT hint silently lost on every expandload.

## Fix

Add `Load->copyMetadata(*CI);` at both per-lane load sites.
