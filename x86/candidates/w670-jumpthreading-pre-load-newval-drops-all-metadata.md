# w670: JumpThreading `simplifyPartiallyRedundantLoad` PRE'd new load drops `!range`, `!nontemporal`, `!invariant.load`, `!invariant.group`, `!noundef`, `!mmra` (and more) — only `AAMetadata` survives

## Pass
`-passes=jump-threading` (default x86 -O2 pipeline includes this).

## Summary

When `simplifyPartiallyRedundantLoad` decides that the load `LoadI` in `LoadBB`
is available on some predecessors but unavailable on exactly one
(`UnavailablePred`), it inserts a brand-new `LoadInst` at the end of
`UnavailablePred` so all preds carry the value and the original load can be
replaced by a PHI. The new load is constructed with only the address, type,
align, ordering and sync-scope from `LoadI`. The **only** metadata copied is
`AAMetadata` (tbaa, !noalias, !alias.scope). Everything else attached to the
original load is silently dropped from the new load.

Concretely the following metadata is lost on the newly-inserted load:

- `!range`
- `!nontemporal`
- `!invariant.load`
- `!invariant.group`
- `!noundef`
- `!mmra`
- `!nonnull` / `!align` / `!dereferenceable` / `!dereferenceable_or_null` (for
  pointer-typed loads)

This is distinct from w262, which is about the *available* predecessor load
(`PredLoadI`) being CSE-combined with `DoesKMove=true`. Here the bug is on
the *unavailable* predecessor where a fresh load is materialized from
scratch and almost every load-metadata kind is dropped.

## Source (LLVM 23.0.0git, `llvm/lib/Transforms/Scalar/JumpThreading.cpp`)

```cpp
// JumpThreading.cpp:1403-1416
if (UnavailablePred) {
  assert(UnavailablePred->getTerminator()->getNumSuccessors() == 1 &&
         "Can't handle critical edge here!");
  LoadInst *NewVal = new LoadInst(
      LoadI->getType(), LoadedPtr->DoPHITranslation(LoadBB, UnavailablePred),
      LoadI->getName() + ".pr", false, LoadI->getAlign(),
      LoadI->getOrdering(), LoadI->getSyncScopeID(),
      UnavailablePred->getTerminator()->getIterator());
  NewVal->setDebugLoc(LoadI->getDebugLoc());
  if (AATags)
    NewVal->setAAMetadata(AATags);          // <-- only AA metadata propagated

  AvailablePreds.emplace_back(UnavailablePred, NewVal);
}
```

`AATags` is the only metadata captured (line 1285). No `getMetadata(MD_range)`
/`MD_nontemporal`/`MD_invariant_load`/etc. is read off `LoadI`, and no
`copyMetadata` / `cloneMetadata` is performed before `NewVal` is wired in as a
PHI input.

## Reproducer

Input `final_a.ll` (canonical pre-load.ll shape, augmented with metadata):

```llvm
target triple = "x86_64-unknown-linux-gnu"

@x = external global i32
@y = external global i32

declare void @f()
declare void @g()

define i32 @pre_drops_metadata(i1 %cond) {
  br i1 %cond, label %A, label %B
A:
  store i32 0, ptr @x
  br label %C
B:
  br label %C
C:
  %ptr = phi ptr [@x, %A], [@y, %B]
  %a = load i32, ptr %ptr, align 8, !range !0, !nontemporal !1, !invariant.load !1, !invariant.group !1, !noundef !1, !mmra !2
  %cond2 = icmp eq i32 %a, 0
  br i1 %cond2, label %YES, label %NO
YES:
  call void @f()
  ret i32 %a
NO:
  call void @g()
  ret i32 1
}
!0 = !{i32 0, i32 100}
!1 = !{}
!2 = !{!"foo", !"bar"}
```

Command:
```
opt -passes=jump-threading -S final_a.ll
```

Actual output (relevant excerpt):
```llvm
A:
  store i32 0, ptr @x, align 4
  br label %YES
C:                                       ; preds = entry
  %a.pr = load i32, ptr @y, align 8       ; <-- ALL metadata dropped!
  %cond2 = icmp eq i32 %a.pr, 0
  br i1 %cond2, label %YES, label %NO
YES:
  %a4 = phi i32 [ 0, %A ], [ %a.pr, %C ]
  call void @f()
  ret i32 %a4
NO:
  call void @g()
  ret i32 1
```

Expected: `%a.pr` should carry `!range !0`, `!nontemporal !1`,
`!invariant.load !1`, `!invariant.group !1`, `!noundef !1`, `!mmra !2` — the
same load metadata that was on `%a` (modulo metadata that's semantically
"per-load" and known unsafe to copy across PRE; none of the listed metadata
falls into that category for this transform).

Notice that the `align 8` attribute (not metadata, a load attribute) DID
survive — confirming that only what's explicitly forwarded survives.

## Why this matters

- `!range` informs LVI/ValueTracking and downstream codegen (e.g.,
  `computeKnownBits`). Losing it on `%a.pr` blocks the same range-based
  simplifications the original `%a` enabled.
- `!nontemporal` selects non-temporal load codegen on x86; losing it on the
  hoisted load defeats the user's explicit hint.
- `!invariant.load` allows hoisting/CSE across calls/stores in later passes;
  dropping it strictly hurts the optimizer.
- `!invariant.group` is required for correct C++ vtable devirtualization in
  later passes — dropping it can de-optimize devirt.
- `!noundef` enables stronger reasoning in InstCombine/SCCP.
- `!mmra` is a memory-model-relaxation annotation that codegen consumes; the
  original load was annotated, the inserted load is not.

For pointer-typed loads `!nonnull`, `!align`,
`!dereferenceable[_or_null]` would also be silently dropped (same code
path).

## Suggested fix

After the `setAAMetadata` call, copy the other load-relevant metadata kinds
from `LoadI` to `NewVal`. The same pattern is used widely elsewhere in LLVM
(see `llvm::combineMetadataForCSE` and `Instruction::copyMetadata` with the
load-safe metadata list):

```cpp
NewVal->copyMetadata(*LoadI, {
    LLVMContext::MD_range,
    LLVMContext::MD_nontemporal,
    LLVMContext::MD_invariant_load,
    LLVMContext::MD_invariant_group,
    LLVMContext::MD_noundef,
    LLVMContext::MD_nonnull,
    LLVMContext::MD_align,
    LLVMContext::MD_dereferenceable,
    LLVMContext::MD_dereferenceable_or_null,
    LLVMContext::MD_mmra,
    LLVMContext::MD_noalias_addrspace,
});
```

(Pick the subset that's semantically safe under the JT-PRE transform; the
ones listed above are all load-site-attached and apply to any access to the
same memory location.)

## Related

- w262 — `combineMetadataForCSE(PredLoadI, LoadI, /*DoesKMove=*/true)` on the
  *available* predecessor load wrongly strips its metadata (different code
  path, line 1459).
- w670 (this) — the *unavailable* predecessor's freshly-created load is
  almost entirely metadata-free (line 1406).
