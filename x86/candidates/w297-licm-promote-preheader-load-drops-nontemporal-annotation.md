# w297 -- LICM `promoteLoopAccessesToScalars` preheader load drops `!nontemporal`, `!annotation`, `!noundef`, `!invariant.load`

## Component
`llvm/lib/Transforms/Scalar/LICM.cpp`, `promoteLoopAccessesToScalars`
at lines 2198-2214.

```cpp
PreheaderLoad =
    new LoadInst(AccessTy, SomePtr, SomePtr->getName() + ".promoted",
                 Preheader->getTerminator()->getIterator());
if (SawUnorderedAtomic)
  PreheaderLoad->setOrdering(AtomicOrdering::Unordered);
PreheaderLoad->setAlignment(Alignment);
PreheaderLoad->setDebugLoc(DebugLoc::getDropped());
if (AATags && LoadIsGuaranteedToExecute)
  PreheaderLoad->setAAMetadata(AATags);
```
(LICM.cpp:2200-2208)

The new preheader load only inherits ordering, alignment, and AA tags
from the original loop loads. Every other metadata flag attached to the
in-loop load(s) is dropped: `!nontemporal`, `!annotation`, `!noundef`,
`!invariant.load`, `!invariant.group`, `!nosanitize`, `!fpmath`, etc.

## Root cause
Same family as w94/w98/w99/w290-w293: a transformed load is constructed
fresh and only a hand-picked subset of metadata is copied. The two
options would have been `copyMetadata` (with an exclusion list for
e.g. `!range` that is now provably-invalid) or `combineMetadataForCSE`
across all `Uses`.

The fix here is analogous to what `cloneInstructionInExitBlock` does
for sinking (line 1410, `New->copyMetadata(*CI)`) -- combine metadata
from the chosen-as-representative loop loads rather than re-implementing
a tiny allowlist.

## Reproducer
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(i64)

define void @prom_ld(ptr noalias dereferenceable(8) align 8 %p, i32 %n) {
entry:
  br label %loop

loop:
  %i   = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %v   = load i64, ptr %p, align 8, !nontemporal !1, !annotation !2
  %add = add i64 %v, 1
  store i64 %add, ptr %p, align 8     ; force the promoter path (not plain hoist)
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
define void @prom_ld(ptr noalias align 8 dereferenceable(8) %p, i32 %n) {
entry:
  %p.promoted = load i64, ptr %p, align 8         ; <-- bare load, no metadata
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
  store i64 %add.lcssa, ptr %p, align 8
  ret void
}
```

Original load: `!nontemporal !1, !annotation !2`.
Hoisted preheader load: neither flag survives. (AA tags would
similarly be dropped if `LoadIsGuaranteedToExecute` is false but
the load is still safe to promote via the dereferenceability path.)

## Why it matters
- `!nontemporal` is a streaming-store/load hint for the backend; the
  promoted preheader load executes once outside the loop but the
  hint was attached to indicate cache-pollution avoidance. Losing it
  is a missed-opt (codegen uses cached MOV instead of MOVNT/VMOVNTDQA
  on x86).
- `!annotation` is observable IR-level metadata used by tooling (e.g.
  hot/cold splitting, custom annotators) and silently disappears,
  breaking the round-trip contract for those downstream passes.
- `!noundef` and `!invariant.load` (when applicable to a single
  representative load) would let downstream passes (InstCombine,
  GVN) hoist further or rule out poison; their loss is also a
  missed-opt.

Note: w94 already covers the syncscope-merge variant of this same
function; this entry tracks the orthogonal metadata-allowlist bug on
the preheader load construction.

## Default-pipeline reachability
`-passes='loop-mssa(licm)'` reproduces; `-O2` runs the loop pipeline.
