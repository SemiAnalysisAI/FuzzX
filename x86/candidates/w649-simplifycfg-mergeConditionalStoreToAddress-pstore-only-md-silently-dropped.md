# w649 - SimplifyCFG `mergeConditionalStoreToAddress` silently drops PStore-only metadata via asymmetric `combineMetadataForCSE`

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` lines 4408-4410
(`mergeConditionalStoreToAddress`):

```cpp
StoreInst *SI = cast<StoreInst>(QB.CreateStore(QPHI, Address));
combineMetadataForCSE(QStore, PStore, true);
SI->copyMetadata(*QStore);
```

`SI` is a *brand new* `StoreInst` with no metadata, inserted into `PostBB`.
The two-step "compute the merged metadata into QStore, then copy it to
SI" pattern relies on `combineMetadataForCSE(K=QStore, J=PStore, DoesKMove=true)`
producing the metadata appropriate for the merged location. But
`combineMetadata` (in `llvm/lib/Transforms/Utils/Local.cpp` lines
2934-3108) only iterates **K**'s metadata:

```cpp
static void combineMetadata(Instruction *K, const Instruction *J,
                            bool DoesKMove, bool AAOnly = false) {
  SmallVector<std::pair<unsigned, MDNode *>, 4> Metadata;
  K->getAllMetadataOtherThanDebugLoc(Metadata);
  for (const auto &MD : Metadata) {
    unsigned Kind = MD.first;
    MDNode *JMD = J->getMetadata(Kind);
    MDNode *KMD = MD.second;
    switch (Kind) {
    default:
      K->setMetadata(Kind, nullptr); // Remove unknown metadata
      break;
    case LLVMContext::MD_tbaa:
      if (DoesKMove)
        K->setMetadata(Kind, MDNode::getMostGenericTBAA(JMD, KMD));
      break;
    ...
```

For every metadata kind exclusively present on `PStore` (the "J" side
that is *not* iterated), no merge is performed and `SI` ends up without
that metadata. The set of kinds silently dropped includes (non-exhaustive,
all reproducible by toggling which side carries the MD):

- `!tbaa`
- `!noalias`, `!alias.scope`
- `!nontemporal`
- `!access_group`, `!mem_parallel_loop_access`
- `!nofpclass`, `!fpmath`
- `!range`, `!nonnull`, `!align`, `!dereferenceable`,
  `!dereferenceable_or_null`
- `!noundef`
- `!nosanitize`, `!noalias_addrspace`

`!invariant.group` is the only kind handled out-of-loop (lines 3065-3067):

```cpp
if (auto *JMD = J->getMetadata(LLVMContext::MD_invariant_group))
  if (isa<LoadInst>(K) || isa<StoreInst>(K))
    K->setMetadata(LLVMContext::MD_invariant_group, JMD);
```

This is actually unsafe in the reverse direction: it picks up PStore's
`!invariant.group` even though the merged store's *value* may now come
from QStore (`select q, QStoreVal, PStoreVal`) on the `q=true` path.
That mismatches the invariant-group contract: the location is being
claimed to invariantly hold whatever the merged select produces, but the
original IR only invariantly promised `PStoreVal` on the PStore-taken
path. See the LangRef "Invariant Group Metadata" section.

`!prof` is also intentionally fixed up separately at lines 4392-4405 of
the SimplifyCFG caller, so that path is independently OK; the silent
drops above are about *all the other* metadata kinds.

## Repros

All three reproduce with bare `opt -passes=simplifycfg -S` (no extra
options). The `simplifycfg-merge-cond-stores` cl::opt defaults to true.

### `repro_pstore_tbaa.ll` — PStore-only `!tbaa`

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define void @mcs_drop_tbaa(i1 %p, i1 %q, ptr %addr) {
entry:
  br i1 %p, label %p.then, label %p.else
p.then:
  store i32 1, ptr %addr, align 4, !tbaa !0
  br label %qbi
p.else:
  br label %qbi
qbi:
  br i1 %q, label %q.then, label %q.else
q.then:
  store i32 2, ptr %addr, align 4
  br label %end
q.else:
  br label %end
end:
  ret void
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C/C++ TBAA"}
```

### `repro_pstore_nontemporal.ll` — PStore-only `!nontemporal`

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define void @mcs_drop_nontemporal(i1 %p, i1 %q, ptr %addr) {
entry:
  br i1 %p, label %p.then, label %p.else
p.then:
  store i32 1, ptr %addr, align 4, !nontemporal !0
  br label %qbi
p.else:
  br label %qbi
qbi:
  br i1 %q, label %q.then, label %q.else
q.then:
  store i32 2, ptr %addr, align 4
  br label %end
q.else:
  br label %end
end:
  ret void
}

!0 = !{i32 1}
```

### `repro_pstore_invariant_group.ll` — PStore-only `!invariant.group` (potential miscompile)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define void @mcs_invariant_group_promoted(i1 %p, i1 %q, ptr %addr) {
entry:
  br i1 %p, label %p.then, label %p.else
p.then:
  store i32 1, ptr %addr, align 4, !invariant.group !0
  br label %qbi
p.else:
  br label %qbi
qbi:
  br i1 %q, label %q.then, label %q.else
q.then:
  store i32 2, ptr %addr, align 4
  br label %end
q.else:
  br label %end
end:
  ret void
}

!0 = !{}
```

## Observed output (TBAA repro)

```
define void @mcs_drop_tbaa(i1 %p, i1 %q, ptr %addr) {
entry:
  %. = select i1 %q, i32 2, i32 1
  %0 = or i1 %p, %q
  br i1 %0, label %1, label %2

1:
  store i32 %., ptr %addr, align 4          ; <-- no !tbaa
  br label %2
2:
  ret void
}
```

## Observed output (invariant.group repro)

```
define void @mcs_invariant_group_promoted(i1 %p, i1 %q, ptr %addr) {
entry:
  %. = select i1 %q, i32 2, i32 1
  %0 = or i1 %p, %q
  br i1 %0, label %1, label %2

1:
  store i32 %., ptr %addr, align 4, !invariant.group !0   ; <-- now claims invariance over `select q, 2, 1`
  br label %2
2:
  ret void
}

!0 = !{}
```

When `p=true` and `q=false`, the merged store writes `1` to `%addr` with
`!invariant.group`. Same as the original PStore — fine.
When `p=false` and `q=true`, the merged store writes `2` to `%addr`
with `!invariant.group`. The *original* IR never set
`!invariant.group` on that path; the location's invariance is now being
asserted with a different value than the source code promised. A
subsequent `load !invariant.group` (or strip.invariant.group / etc.) at
the same address can read `2` and propagate that as a load-invariant
value into a context where the source semantics did *not* permit such
propagation.

Tagging the merged store with `!invariant.group` is unsound whenever the
two source stores don't already agree on the stored value on all paths
where invariant.group was carried.

## Fix

1. Make `combineMetadata` symmetric — iterate `J`'s metadata too and
   for every kind, perform the per-kind merge (which for the J-only,
   K-empty case usually means "drop", but the call site is what
   currently determines the outcome).

2. Or, more locally, in `mergeConditionalStoreToAddress`, *do not* use
   `combineMetadataForCSE` (whose contract is "K is staying put, J is
   being deleted"). Instead, manually merge per-kind into SI with the
   correct "this is a freshly speculated store" semantics, including
   the `!invariant.group` rule that requires the value to be the same
   on both incoming sides before claiming invariance.

The current pattern of "mutate QStore, then copy to SI" also obscures
the intent — SI is the moved instruction, not QStore — and makes
review of metadata correctness harder than it would be if SI were the
target of the combine directly.
