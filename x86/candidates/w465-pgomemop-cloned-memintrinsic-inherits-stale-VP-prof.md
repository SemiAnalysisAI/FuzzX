# w465: PGOMemOPSizeOpt cloned size-specialized memintrinsic inherits stale !prof (value-profile) metadata from original

File: `llvm/lib/Transforms/Instrumentation/PGOMemOPSizeOpt.cpp`
Lines: 393, 397-399, 410, 416

## Summary

`MemOPSizeOpt::perform` splits a `memcpy`/`memset` whose length is value-profiled
into a switch with one BB per hot size plus a default BB that holds the
original call. The original's `!prof` value-profile is cleared on line 393,
then optionally re-annotated with the *remaining* (un-promoted) VP entries on
lines 395-399 via `annotateValueSite`:

```cpp
// Clear the value profile data.
MO.I->setMetadata(LLVMContext::MD_prof, nullptr);
// If all promoted, we don't need the MD.prof metadata.
if (SavedRemainCount > 0 || Version != VDs.size()) {
  // Otherwise we need update with the un-promoted records back.
  annotateValueSite(*Func.getParent(), *MO.I, RemainingVDs, SavedRemainCount,
                    IPVK_MemOPSize, VDs.size());
}
```

The per-size clone loop then runs:

```cpp
for (uint64_t SizeId : SizeIds) {
  BasicBlock *CaseBB = BasicBlock::Create(...);
  MemOp NewMO = MO.clone();              // <-- copies ALL metadata, including !prof
  ...
  NewMO.I->insertInto(CaseBB, CaseBB->end());
  ...
}
```

`MemOp::clone()` (lines 122-126) delegates to `Instruction::clone()`
(`llvm/lib/IR/Instruction.cpp:1464`), which copies metadata via
`New->copyMetadata(*this)` (line 1478). So every per-size specialized call in
`MemOP.Case.<sz>` inherits the **VP metadata that was re-annotated onto the
default call** — even though those VP entries describe sizes other than
`SizeId`.

The size-specialized clones carry no meaningful !prof value-profile and
should have it stripped before insertion, but the current order
(clear -> re-annotate -> clone) leaks the per-default VP data onto every
case BB clone.

## Repro

```llvm
; RUN: opt -passes=pgo-memop-opt -S %s

target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.memcpy.p0.p0.i64(ptr noalias nocapture writeonly, ptr noalias nocapture readonly, i64, i1 immarg)

define void @test(ptr %dst, ptr %src, i64 %sz) !prof !1 {
entry:
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %src, i64 %sz, i1 false), !prof !2
  ret void
}

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"ProfileSummary", !3}
!1 = !{!"function_entry_count", i64 100000}
!2 = !{!"VP", i32 1, i64 100000, i64 16, i64 60000, i64 32, i64 8000, i64 64, i64 5000, i64 128, i64 3000}
!3 = !{!4, !5, !6, !7, !8, !9, !10, !11}
!4 = !{!"ProfileFormat", !"InstrProf"}
!5 = !{!"TotalCount", i64 100000}
!6 = !{!"MaxCount", i64 100000}
!7 = !{!"MaxInternalCount", i64 100000}
!8 = !{!"MaxFunctionCount", i64 100000}
!9 = !{!"NumCounts", i64 1}
!10 = !{!"NumFunctions", i64 1}
!11 = !{!"DetailedSummary", !12}
!12 = !{!13}
!13 = !{i32 999000, i64 100000, i32 1}
```

Output (relevant fragment):

```llvm
MemOP.Case.16:
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %src, i64 16, i1 false), !prof !14
  br label %MemOP.Merge

MemOP.Default:
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %src, i64 %sz, i1 false), !prof !14
  br label %MemOP.Merge

!14 = !{!"VP", i32 1, i64 40000, i64 32, i64 8000, i64 64, i64 5000, i64 128, i64 3000}
```

The `MemOP.Case.16` clone (length is the constant `i64 16`) carries a VP
record claiming the size distribution is `{32:8000, 64:5000, 128:3000}` —
that record describes the *default* branch's remaining size distribution,
not the size-16 case. Any downstream analysis that consults this VP (other
PGOMemOPSizeOpt-aware passes, LTO PGO use, summary builders, value-profile
verifiers) will read nonsensical data: a memcpy with a constant length of
16 advertising frequencies of length 32/64/128.

## Why it's a bug pattern match

"PGOMemOPSizeOpt wrong split fallback" — the per-case clones are part of
the split fallback and are emitted with an incorrect !prof attribute that
contradicts the actual constant length stored in the clone. The fix is to
clear `MD_prof` on `NewMO.I` after cloning (or to reorder: clear-prof, clone
N times, re-annotate the default only after all clones are made).
