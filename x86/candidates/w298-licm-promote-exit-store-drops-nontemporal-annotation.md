# w298 -- LICM `LoopPromoter::insertStoresInLoopExitBlocks` drops `!nontemporal`, `!annotation`, `!nosanitize` on sunk exit-block store

## Component
`llvm/lib/Transforms/Scalar/LICM.cpp`,
`LoopPromoter::insertStoresInLoopExitBlocks` at lines 1813-1860.

```cpp
StoreInst *NewSI = new StoreInst(LiveInValue, Ptr, InsertPos);
if (UnorderedAtomic)
  NewSI->setOrdering(AtomicOrdering::Unordered);
NewSI->setAlignment(Alignment);
NewSI->setDebugLoc(DL);
// Attach DIAssignID metadata to the new store ...
if (i == 0) {
  NewSI->mergeDIAssignID(Uses);
  NewID = cast_or_null<DIAssignID>(
      NewSI->getMetadata(LLVMContext::MD_DIAssignID));
} else {
  NewSI->setMetadata(LLVMContext::MD_DIAssignID, NewID);
}

if (AATags)
  NewSI->setAAMetadata(AATags);
```
(LICM.cpp:1825-1846)

The exit-block store inherits ordering, alignment, debug location,
`DIAssignID`, and AA tags. Every other piece of metadata that was on
the in-loop store(s) is silently discarded.

## Root cause
Same allowlist-style construction as w297 but on the store side. The
function builds a fresh `StoreInst` and only copies an explicit handful
of attachments. There is no call to either `copyMetadata` or to
`combineMetadataForCSE` across the `Uses` array (the in-loop stores
being merged), so:

- `!nontemporal` -- streaming-store hint lost on the only store that
  actually still touches memory.
- `!annotation` -- tooling/IR-annotation metadata lost.
- `!nosanitize` -- silently dropped, can affect sanitizer
  instrumentation downstream.
- `!noundef` (on store value semantics for tooling),
  `!mem_parallel_loop_access`, `!access_group` -- also gone.

## Reproducer
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(i64)

define void @sink2(ptr noalias dereferenceable(8) align 8 %p, i32 %n) {
entry:
  br label %loop

loop:
  %i      = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %v      = load i64, ptr %p, align 8
  %add    = add i64 %v, 1
  store i64 %add, ptr %p, align 8, !nontemporal !1, !annotation !2
  %i.next = add nsw i32 %i, 1
  %cmp    = icmp slt i32 %i.next, %n
  br i1 %cmp, label %loop, label %exit

exit:
  ret void
}

!1 = !{i32 1}
!2 = !{!"my_annotation"}
```

`opt -passes='loop-mssa(licm)' -S` (LLVM 23.0.0git x86):
```ll
define void @sink2(ptr noalias align 8 dereferenceable(8) %p, i32 %n) {
entry:
  %p.promoted = load i64, ptr %p, align 8
  br label %loop

loop:                                             ; preds = %loop, %entry
  %add1   = phi i64 [ %p.promoted, %entry ], [ %add, %loop ]
  %i      = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %add    = add i64 %add1, 1
  %i.next = add nsw i32 %i, 1
  %cmp    = icmp slt i32 %i.next, %n
  br i1 %cmp, label %loop, label %exit

exit:                                             ; preds = %loop
  %add.lcssa = phi i64 [ %add, %loop ]
  store i64 %add.lcssa, ptr %p, align 8           ; <-- !nontemporal/!annotation gone
  ret void
}
```

## Why it matters
- The in-loop store was decorated `!nontemporal` (deliberate
  cache-bypass for a write-heavy hot loop). After promotion, the only
  store that still hits memory is the sunk exit-block store, and it
  has no `!nontemporal` -- so codegen falls back to a cached MOV
  instead of the requested MOVNT, the exact opposite of the user's
  intent.
- `!annotation` propagation is needed for downstream tooling
  (hot/cold splitting, custom passes).
- `!nosanitize` loss can re-enable sanitizer instrumentation on the
  exit store, masking pre-marked safe-store invariants.

Note: w94 covers the syncscope-merge variant for promoted
load/store of mixed scopes; this entry tracks the orthogonal
metadata-allowlist bug for the exit-block store construction.

## Default-pipeline reachability
`-passes='loop-mssa(licm)'` reproduces; the promoter is enabled by
default and fires whenever the conditions in
`promoteLoopAccessesToScalars` are met -- standard `-O2` LICM run.
