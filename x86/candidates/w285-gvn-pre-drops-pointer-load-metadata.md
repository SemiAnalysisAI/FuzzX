# w285: GVN PRE drops `!nonnull`, `!dereferenceable`, `!align`, `!noundef`, `!nontemporal`, `!fpmath` on PRE-inserted load

**Severity:** Missed optimization / soft soundness (lost metadata-derived facts).

**Where:** `llvm/lib/Transforms/Scalar/GVN.cpp:1565-1604`
(file path: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Transforms/Scalar/GVN.cpp`)

## Root cause

`GVNPass::eliminatePartiallyRedundantLoad` constructs a brand-new load
(`NewLoad`) in each unavailable predecessor and then copies a hard-coded
subset of metadata from the original `Load` onto it:

```cpp
1587:    // Transfer the old load's AA tags to the new load.
1588:    AAMDNodes Tags = Load->getAAMetadata();
1589:    if (Tags)
1590:      NewLoad->setAAMetadata(Tags);
1591:
1592:    if (auto *MD = Load->getMetadata(LLVMContext::MD_invariant_load))
1593:      NewLoad->setMetadata(LLVMContext::MD_invariant_load, MD);
1594:    if (auto *InvGroupMD = Load->getMetadata(LLVMContext::MD_invariant_group))
1595:      NewLoad->setMetadata(LLVMContext::MD_invariant_group, InvGroupMD);
1596:    if (auto *RangeMD = Load->getMetadata(LLVMContext::MD_range))
1597:      NewLoad->setMetadata(LLVMContext::MD_range, RangeMD);
1598:    if (auto *NoFPClassMD = Load->getMetadata(LLVMContext::MD_nofpclass))
1599:      NewLoad->setMetadata(LLVMContext::MD_nofpclass, NoFPClassMD);
1600:
1601:    if (auto *AccessMD = Load->getMetadata(LLVMContext::MD_access_group))
1602:      if (LI->getLoopFor(Load->getParent()) == LI->getLoopFor(UnavailableBlock))
1603:        NewLoad->setMetadata(LLVMContext::MD_access_group, AccessMD);
```

The whitelist covers: AAMD (tbaa, alias.scope, noalias, noalias_addrspace),
`!invariant_load`, `!invariant_group`, `!range`, `!nofpclass`, conditional
`!access_group`.

The whitelist **omits all of**:

| Metadata kind                  | Applies to load? |
| ------------------------------ | ---------------- |
| `!nonnull`                     | yes (ptr load)   |
| `!dereferenceable`             | yes (ptr load)   |
| `!dereferenceable_or_null`     | yes (ptr load)   |
| `!align`                       | yes (ptr load)   |
| `!noundef`                     | yes (any load)   |
| `!nontemporal`                 | yes (any load)   |
| `!fpmath`                      | yes (fp load)    |

Each of these is preserved by `combineMetadataForCSE` in the local-CSE path,
so this is an asymmetry: same load, same metadata, but PRE silently strips
it while CSE intersects it.

The PRE result is `phi [%old_load_from_avail_pred, %newly_inserted_load]`.
The eliminated `Load`'s metadata never reappears on the SSA value users see
— neither the in-predecessor available load nor the new PRE load carries it.
This regresses any optimization that depends on these facts.

## Reproducer (pointer load: `!nonnull`, `!dereferenceable`, `!align`, `!noundef` all dropped)

```ll
; opt -passes=gvn -S
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @clobber(ptr)

define ptr @pre(i1 %c, ptr %p, ptr noalias %q) {
entry:
  %a = load ptr, ptr %p, align 8
  br i1 %c, label %then, label %else

then:
  call void @clobber(ptr %q)
  br label %merge

else:
  br label %merge

merge:
  %v = load ptr, ptr %p, align 8, !nonnull !0, !dereferenceable !1, !align !2, !noundef !0
  %z = getelementptr inbounds i8, ptr %v, i64 16
  store i8 0, ptr %z
  ret ptr %a
}

!0 = !{}
!1 = !{i64 32}
!2 = !{i64 8}
```

`opt -passes=gvn -S` yields:

```ll
then:
  call void @clobber(ptr %q)
  %v.pre = load ptr, ptr %p, align 8           ; <-- NO metadata, lost
  br label %merge

else:
  br label %merge

merge:
  %v = phi ptr [ %a, %else ], [ %v.pre, %then ]
  %z = getelementptr inbounds i8, ptr %v, i64 16
  store i8 0, ptr %z, align 1
  ret ptr %a
```

The `%v.pre` load has none of the original `%v`'s `!nonnull`,
`!dereferenceable !1 = !{i64 32}`, `!align !2 = !{i64 8}`, `!noundef`
metadata. The phi result naturally cannot carry them either. Downstream
passes can no longer prove the loaded pointer is non-null, 32-byte
dereferenceable, 8-byte aligned, or non-undef.

## Reproducer (FP load: `!fpmath` dropped)

```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @clobber(ptr)

define float @pre(i1 %c, ptr %p, ptr noalias %q) {
entry:
  %a = load float, ptr %p, align 4
  br i1 %c, label %then, label %else

then:
  call void @clobber(ptr %q)
  br label %merge

else:
  br label %merge

merge:
  %v = load float, ptr %p, align 4, !fpmath !0
  %sum = fadd float %v, %a
  ret float %sum
}

!0 = !{float 2.5}
```

After `opt -passes=gvn -S`, `%v.pre = load float, ptr %p, align 4` (no
`!fpmath`); downstream `fadd` no longer knows it can tolerate 2.5 ULPs of
error.

## Reproducer (`!nontemporal` dropped)

Same diamond shape; the original load carries `!nontemporal !{i32 1}`. The
PRE-inserted load drops it. Result: a normally-cached load runs in the cold
predecessor and the non-temporal store-bypass hint is silently lost.

## Suggested fix

Replace the whitelisted-copy loop with a call to `copyMetadataForLoad`
(`Local.cpp:3119`) which is the canonical load-metadata copy used by SROA /
LoopVectorizer, or extend the whitelist to include
`MD_nonnull, MD_dereferenceable, MD_dereferenceable_or_null, MD_align,
MD_noundef, MD_nontemporal, MD_fpmath`. The semantics are correct: the new
load is from the same pointer at a strictly-dominating point, so any
guarantee the original site held still holds at the new site.

## Default x86 -O2 only

Reproduces with `opt -passes=gvn -S` on default `x86_64-unknown-linux-gnu`;
no source-level changes required.
