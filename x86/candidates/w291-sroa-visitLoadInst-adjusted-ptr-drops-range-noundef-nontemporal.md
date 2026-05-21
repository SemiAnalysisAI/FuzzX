# w291 -- SROA `visitLoadInst` (adjusted-pointer path) drops `!range`, `!noundef`, `!nontemporal`, `!invariant.load` on the rewritten load

## Component
`llvm/lib/Transforms/Scalar/SROA.cpp`, `AllocaSliceRewriter::visitLoadInst`,
the "else" arm (adjusted-pointer / non-convertible type) at lines 3195-3212.

## Root cause
When SROA cannot convert the new alloca type directly to the load type
(line 3158's `canConvertValue` is false) it falls through to:

```cpp
LoadInst *NewLI =
    IRB.CreateAlignedLoad(TargetTy, getNewAllocaSlicePtr(IRB, LTy),
                          getSliceAlign(), LI.isVolatile(), LI.getName());

if (AATags)
  NewLI->setAAMetadata(AATags.adjustForAccess(
      NewBeginOffset - BeginOffset, NewLI->getType(), DL));

if (LI.isVolatile())
  NewLI->setAtomic(LI.getOrdering(), LI.getSyncScopeID());
NewLI->copyMetadata(LI, {LLVMContext::MD_mem_parallel_loop_access,
                         LLVMContext::MD_access_group});
```
(SROA.cpp:3197-3208)

The only metadata copied is `MD_mem_parallel_loop_access`, `MD_access_group`
plus AA tags (handled separately). Everything else is dropped, including
`!range`, `!noundef`, `!nontemporal`, `!invariant.load`, `!nonnull`, `!align`,
`!dereferenceable`, `!fpmath`, `!nofpclass`, `!noalias_addrspace`.

The companion `rewriteIntegerLoad` (lines 3115-3137) is even worse: it
creates a fresh load of the *alloca* type and does not copy any metadata
from the original LI at all (no AA, no parallel-loop, no access-group).

The same `copyMetadata({MD_mem_parallel_loop_access, MD_access_group})`
pattern is repeated in `rewriteVectorizedLoadInst` (line 3110) and
`presplitLoadsAndStores` for the split loads (line 4847).

These are the same kinds of metadata that `llvm::copyMetadataForLoad`
(Local.cpp:3119-3177) is the canonical helper for.

## Reproducer
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(i32, float)

define void @test(i64 %x) {
entry:
  ; [12 x i8] alloca - won't fully promote to scalar
  %a = alloca [12 x i8], align 4
  store i64 %x, ptr %a, align 4
  %p = getelementptr i8, ptr %a, i64 8
  store i32 7, ptr %p, align 4
  %p1 = getelementptr i8, ptr %a, i64 2
  %r1 = load i32, ptr %p1, align 2, !range !0, !noundef !1, !nontemporal !2
  %p2 = getelementptr i8, ptr %a, i64 6
  %r2 = load float, ptr %p2, align 2, !nontemporal !2
  call void @use(i32 %r1, float %r2)
  ret void
}

!0 = !{i32 0, i32 1024}
!1 = !{}
!2 = !{i32 1}
```

`opt -passes=sroa -S`:
```ll
define void @test(i64 %x) {
entry:
  %a = alloca [12 x i8], align 4
  store i64 %x, ptr %a, align 4
  %a.8.p.sroa_idx = getelementptr inbounds i8, ptr %a, i64 8
  store i32 7, ptr %a.8.p.sroa_idx, align 4
  %a.2.p1.sroa_idx = getelementptr inbounds i8, ptr %a, i64 2
  %a.2.r1 = load i32, ptr %a.2.p1.sroa_idx, align 2     ; <-- !range,!noundef,!nontemporal gone
  %a.6.p2.sroa_idx = getelementptr inbounds i8, ptr %a, i64 6
  %a.6.r2 = load float, ptr %a.6.p2.sroa_idx, align 2   ; <-- !nontemporal gone
  call void @use(i32 %a.2.r1, float %a.6.r2)
  ret void
}
```

All three metadata are silently lost on `%a.2.r1` and `!nontemporal` on
`%a.6.r2`. The pointer arithmetic and load type are unchanged from the
original (semantically equivalent) -- there is no reason to drop the load
metadata.

## Impact
- `!nontemporal` loss is a documented codegen regression: x86 lowering would
  have used `MOVNTDQA` / streaming-load hints; now emits a plain load.
- `!range` loss defeats downstream value-range based folding (InstCombine,
  CVP, GVN).
- `!noundef` loss prevents `freeze`-removal and other poison-reasoning
  optimizations.
- `!invariant.load` loss can prevent hoisting/CSE that depended on the
  freshness guarantee.

## Fix sketch
Use `copyMetadataForLoad(*NewLI, LI)` (`Transforms/Utils/Local.h:434`) right
after creating `NewLI` in both the slice-ptr arm (line 3197) and the
non-adjusted-ptr arm (line 3164), and inside `rewriteIntegerLoad` /
`rewriteVectorizedLoadInst` / `presplitLoadsAndStores` split loads. AA tags
must still be `adjustForAccess`-adjusted afterwards (the slice may shift the
TBAA offset).

## Notes
- Default x86 -O2 only. Confirmed on LLVM 23.0.0git (FuzzX `opt` build).
- Distinct from w61/w78 (atomic flag drops on SROA load/store), distinct
  from w97 (memcpy-split TBAA override), distinct from w106 (InstCombine
  load-of-select).
