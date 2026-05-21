# 202 — ScalarizeMaskedMemIntrin `scalarizeMaskedGather` dynamic-mask path drops ALL metadata on per-lane loads

Component: `llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp` lines ~517-574 (`scalarizeMaskedGather`)

Line 557-558: `LoadInst *Load = Builder.CreateAlignedLoad(EltTy, Ptr, AlignVal, ...)` followed immediately by `CreateInsertElement` — there is no `Load->copyMetadata(*CI)` call. (The whole file has 4 `copyMetadata` calls, all for `scalarizeMaskedLoad/Store` — never for gather/scatter/expand/compress.)

Distinct from #180 (constant-mask fast paths in the same file): this is the dynamic-mask path that fires on typical vectorized loops with runtime masks.

## Downstream-observable consequence

With `!range [0,2)` on the gather, `opt -passes=instcombine` alone folds `icmp uge %e, 2` to `false`. After running `scalarize-masked-mem-intrin` first, the per-lane loads have no `!range`, instcombine cannot fold, and an avoidable compare/branch survives in the output IR.

## Severity

Default x86 -O2 (this pass is in the default CodeGen pipeline). Per-lane loss also of `!nontemporal`/`!noalias`/`!alias.scope`/`!nonnull`/`!dereferenceable`/`!invariant.load`/`!invariant.group`/TBAA.

## Fix

After line 558 add `Load->copyMetadata(*CI);`.
