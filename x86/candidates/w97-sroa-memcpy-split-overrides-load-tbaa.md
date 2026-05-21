# w97: SROA memcpy-split overrides per-load !tbaa/!nontemporal with memcpy's metadata

## Component
`llvm/lib/Transforms/Scalar/SROA.cpp` — `visitMemTransferInst` rewrite path that
splits a memcpy into per-slice `load`/`store` pairs (around lines 3749 and 3772).

## Root cause
When SROA rewrites `memcpy(alloca, src)` plus subsequent loads from the alloca by
splitting the memcpy into per-slice copies *and* eliminating the alloca, the new
loads-from-src inherit:
- `AATags` from the **memcpy** (via `AATags = II.getAAMetadata()`), and
- only `MD_mem_parallel_loop_access` + `MD_access_group` via
  `Load->copyMetadata(II, {...})` at line 3751:
```cpp
Load->copyMetadata(II, {LLVMContext::MD_mem_parallel_loop_access,
                        LLVMContext::MD_access_group});
if (AATags)
  Load->setAAMetadata(AATags.adjustForAccess(...));
```
The **original loads' per-load `!tbaa`, `!nontemporal`, `!invariant.load`, `!noundef`,
`!range`, `!nonnull`** are *all* dropped — there is no merging with the user-load
metadata. The new loads silently adopt the memcpy's (broad, often "any pointer") TBAA.

This is wrong because:
1. The original loads' TBAA was *more specific* (e.g. "int") than the memcpy's
   `"any pointer"`. After SROA the loads claim looser type info.
2. `!nontemporal` (a real codegen directive — drives MOVNT on x86, weak ordering)
   is dropped.
3. `!invariant.load` and `!noundef` from user loads are dropped, weakening
   downstream GVN/LICM hoisting decisions and UB detection.

## opt diff (reproducible with `opt -passes=sroa -S`)
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i64 @test(ptr %src) {
entry:
  %a = alloca [16 x i8], align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %a, ptr align 8 %src, i64 16, i1 false), !tbaa !2
  %v0 = load i64, ptr %a,                            align 8, !nontemporal !0, !tbaa !4
  %p1 = getelementptr inbounds i8, ptr %a, i64 8
  %v1 = load i64, ptr %p1,                            align 8, !nontemporal !0, !tbaa !6
  %sum = add i64 %v0, %v1
  ret i64 %sum
}

declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)

!0 = !{i32 1}
!1 = !{!"root"}
!2 = !{!3, !3, i64 0}     ; memcpy: "any pointer"
!3 = !{!"any pointer", !1, i64 0}
!4 = !{!5, !5, i64 0}     ; v0:     "int"
!5 = !{!"int", !1, i64 0}
!6 = !{!7, !7, i64 0}     ; v1:     "float"
!7 = !{!"float", !1, i64 0}
```
Output:
```ll
define i64 @test(ptr %src) {
entry:
  %a.sroa.0.0.copyload = load i64, ptr %src, align 8, !tbaa !0          ; "any pointer"
  %a.sroa.2.0.src.sroa_idx = getelementptr inbounds i8, ptr %src, i64 8
  %a.sroa.2.0.copyload     = load i64, ptr %a.sroa.2.0.src.sroa_idx, align 8, !tbaa !0
  %sum = add i64 %a.sroa.0.0.copyload, %a.sroa.2.0.copyload
  ret i64 %sum
}
!0 = !{!1, !1, i64 0}
!1 = !{!"any pointer", !2, i64 0}
!2 = !{!"root"}
```
Result:
- Both loads have **`!tbaa "any pointer"`** (memcpy's tag) — the per-load `"int"`
  and `"float"` tags are gone.
- `!nontemporal !0` is **dropped** from both loads. On x86 this changes a
  cache-bypassing MOVNT load (weak ordering) into a regular cached MOV
  (stronger ordering / cacheable behavior). User-visible memory-ordering
  semantics change.

## Why this is a miscompile (not just sub-optimal)
- `!nontemporal` is consumed by `SelectionDAGBuilder` (line 4961/5119) to set
  `MOLoad/Store::MONonTemporal`, which on x86 lowers to MOVNT* variants with
  **non-temporal** semantics (weak ordering w.r.t. other stores — programmer-visible
  via `_mm_sfence`/`_mm_lfence` pairing). Dropping it on user-visible loads
  silently strengthens ordering, breaking SC-DRF guarantees the user encoded.
- Adopting the memcpy's `!tbaa` on user loads is a tag *substitution* that can
  flip later AA queries either direction (here looser; in reverse cases
  tighter, which can let GVN/MemDep delete a partially-aliasing store).

## Fix sketch
- Use the user-load's metadata (not the memcpy's) when SROA eliminates the
  alloca by inlining per-slice loads-from-source — the alloca's loads are the
  observers, the memcpy is just the value-flow.
- Or: explicitly merge (intersect) the memcpy's AATags with the user-load's
  AATags via `MDNode::getMostGenericTBAA`, and preserve `!nontemporal` from
  *either* source.

## Notes
- Distinct from w78-sroa-{tree-merge,vector-promotion}-drops-atomic
  (those handle the atomic flag drop). This is metadata-substitution and
  `!nontemporal` drop on the memcpy-rewrite path.
- Repro on LLVM 23.0.0git (FuzzX `opt` build).
