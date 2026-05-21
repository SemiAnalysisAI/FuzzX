# w106: InstCombine load-of-select fold drops `!invariant.group`, `!nontemporal`, `!tbaa`, `!dereferenceable`

**File:** `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp`
**Function:** `InstCombinerImpl::visitLoadInst` load-of-select arm, ~line 1144-1178.

## Root cause

When the load is fed by a select and both arms are safe to load
unconditionally, InstCombine rewrites `load (select c, %p1, %p2)` to
`select c, (load %p1), (load %p2)`. The newly created loads `V1`/`V2` only
receive:

```cpp
V1->copyMetadata(LI, Metadata::PoisonGeneratingIDs);
V2->copyMetadata(LI, Metadata::PoisonGeneratingIDs);
```

where `Metadata::PoisonGeneratingIDs` (Metadata.h:146) is only
`{MD_range, MD_nonnull, MD_align, MD_nofpclass}`.

Everything else is silently dropped, including **non-poison** but
semantically-important load metadata:
- `!invariant.group` (a same-group peer load could now be CSE'd across a peer
  store with no invariant.group, losing devirt safety)
- `!invariant.load`
- `!tbaa`, `!alias.scope`, `!noalias` (AA pessimization, but also can cause
  miscompiles when downstream passes rely on the TBAA promise the source IR
  documented)
- `!nontemporal`
- `!dereferenceable`, `!dereferenceable_or_null`
- `!noalias.addrspace`, `!noundef`

The known bug #165 already covers `!noundef`. This finding shows the much
broader set of metadata that is dropped at the same call site, including
non-UB metadata — distinct from #163/#168 which are in the unpack/retype paths.

## Reproducer

```llvm
; opt -passes=instcombine -S
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"

define i64 @f(i1 %c, ptr dereferenceable(8) align 8 %p1,
              ptr dereferenceable(8) align 8 %p2) {
  %p = select i1 %c, ptr %p1, ptr %p2
  %v = load i64, ptr %p, align 8,
       !invariant.group !0, !nontemporal !1, !tbaa !4
  ret i64 %v
}
!0 = !{}
!1 = !{i32 1}
!2 = !{!"tbaa root"}
!3 = !{!"int", !2, i64 0}
!4 = !{!3, !3, i64 0}
```

### Before
```
%p = select i1 %c, ptr %p1, ptr %p2
%v = load i64, ptr %p, align 8, !invariant.group !0, !nontemporal !1, !tbaa !4
```

### After (opt diff)
```
%p1.val = load i64, ptr %p1, align 8     ; NO metadata at all
%p2.val = load i64, ptr %p2, align 8     ; NO metadata at all
%v      = select i1 %c, i64 %p1.val, i64 %p2.val
```

A second reproducer with `!dereferenceable !0 = !{i64 16}` on a `load ptr`
shows `!dereferenceable` is similarly dropped.

## Why this is a miscompile (not just QoI)

`!invariant.group` and `!invariant.load` are *not* poison-generating — they are
optimization-enabling guarantees. A subsequent pass that sees two peer loads
in the same invariant.group may CSE them, but the same pass would refuse to
CSE across a peer store that lacks the group token. By stripping the group
token off the load, the pair no longer matches the peer load that *did* keep
its group annotation in a sibling block, breaking the invariance the source
intended. The result is wrong devirtualized targets after vptr placement-new.

`!nontemporal` dropping is a perf miscompile only; `!tbaa`/`!noalias`
dropping is conservative for the immediate load but can let downstream
loop-aware passes (LICM, GVN-PRE) wrongly conclude two accesses may alias
and reorder them.

## Fix

In the load-of-select arm of `InstCombinerImpl::visitLoadInst`, replace the
`copyMetadata(LI, PoisonGeneratingIDs)` calls with a full
`copyMetadataForLoad(*V1, LI); copyMetadataForLoad(*V2, LI);` (factor the
existing helper used for retype). All metadata listed in `copyMetadataForLoad`
is safe to clone onto loads that perform the *same access* in different
control-flow paths, since each cloned load happens iff the original would have
executed in that arm.
