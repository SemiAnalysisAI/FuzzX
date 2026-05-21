# `LoopUnrollAndJam` drops `DebugLoc` on replaced sub-loop / aft-loop intermediate terminators

## File and root cause

`llvm/lib/Transforms/Utils/LoopUnrollAndJam.cpp` — three sites where a
conditional branch is replaced with a fresh `UncondBrInst` without
copying the source `DebugLoc` of the removed instruction:

```cpp
// LoopUnrollAndJam.cpp:519-526 — subloop intermediate branches.
for (unsigned It = 1; It != Count; It++) {
  // Replace the conditional branch of the previous iteration subloop with an
  // unconditional one to this one
  CondBrInst *SubTerm =
      cast<CondBrInst>(SubLoopBlocksLast[It - 1]->getTerminator());
  UncondBrInst::Create(SubLoopBlocksFirst[It], SubTerm->getIterator());
  SubTerm->eraseFromParent();                       // <-- no setDebugLoc
  ...
}

// LoopUnrollAndJam.cpp:534-538 — aft-blocks final branch on CompletelyUnroll.
CondBrInst *AftTerm = cast<CondBrInst>(AftBlocksLast.back()->getTerminator());
if (CompletelyUnroll) {
  UncondBrInst::Create(LoopExit, AftTerm->getIterator());
  AftTerm->eraseFromParent();                       // <-- no setDebugLoc
}

// LoopUnrollAndJam.cpp:547-554 — aft-blocks intermediate branches.
for (unsigned It = 1; It != Count; It++) {
  CondBrInst *AftTerm =
      cast<CondBrInst>(AftBlocksLast[It - 1]->getTerminator());
  UncondBrInst::Create(AftBlocksFirst[It], AftTerm->getIterator());
  AftTerm->eraseFromParent();                       // <-- no setDebugLoc
  ...
}
```

In all three sites a fresh `UncondBrInst::Create(...)` is constructed,
the old conditional branch's iterator is reused as the insertion point,
and the old branch is erased. None of the three call sites preserves the
source debug location of the erased terminator. The new `UncondBr` is
born with no debug location at all.

Compare the analogous code in `LoopUnroll.cpp`, which got this right in
`SetDest` (line 1346-1358):

```cpp
// LoopUnroll.cpp:1354-1357
auto *BI = UncondBrInst::Create(Dest, Term->getIterator());
BI->setDebugLoc(Term->getDebugLoc());                // <-- preserves the source loc
Term->eraseFromParent();
```

`LoopUnrollAndJam` and `LoopUnroll` are sister utilities, both replace
the same kind of terminator under the same circumstance (an iteration's
back/exit edge is rewired to be unconditional), and they should be
behaving identically with respect to debug-info preservation.

## Reproducer

`/tmp/unroll-test/test-jam-dbg.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @jam_dbg(ptr noalias %p, ptr noalias %q) !dbg !4 {
entry:
  br label %outer.loop, !dbg !10

outer.loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %outer.latch ]
  br label %inner.loop, !dbg !11

inner.loop:
  %j = phi i32 [ 0, %outer.loop ], [ %j.next, %inner.loop ]
  %idx = mul i32 %i, 8, !dbg !12
  %idx2 = add i32 %idx, %j, !dbg !12
  %gep = getelementptr i32, ptr %p, i32 %idx2, !dbg !12
  %v = load i32, ptr %gep, !dbg !12
  %gep2 = getelementptr i32, ptr %q, i32 %idx2, !dbg !12
  store i32 %v, ptr %gep2, !dbg !12
  %j.next = add i32 %j, 1, !dbg !13
  %j.cmp = icmp ult i32 %j.next, 8, !dbg !13
  ; Inner sub-latch with a real debug location:
  br i1 %j.cmp, label %inner.loop, label %outer.latch, !dbg !14

outer.latch:
  %i.next = add i32 %i, 1
  %i.cmp = icmp ult i32 %i.next, 100
  br i1 %i.cmp, label %outer.loop, label %exit, !llvm.loop !20

exit:
  ret void
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!1, !2}
!0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !3, isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !{i32 2, !"Dwarf Version", i32 5}
!2 = !{i32 2, !"Debug Info Version", i32 3}
!3 = !DIFile(filename: "x.c", directory: "/tmp")
!4 = distinct !DISubprogram(name: "jam_dbg", scope: !3, file: !3, line: 1, type: !5, isLocal: false, isDefinition: true, scopeLine: 1, isOptimized: true, unit: !0)
!5 = !DISubroutineType(types: !6)
!6 = !{null}
!10 = !DILocation(line: 2, column: 1, scope: !4)
!11 = !DILocation(line: 3, column: 1, scope: !4)
!12 = !DILocation(line: 4, column: 1, scope: !4)
!13 = !DILocation(line: 5, column: 1, scope: !4)
!14 = !DILocation(line: 6, column: 1, scope: !4)
!20 = distinct !{!20, !{!"llvm.loop.unroll_and_jam.enable"}, !{!"llvm.loop.unroll_and_jam.count", i32 4}}
```

### `opt -passes=loop-unroll-and-jam -allow-unroll-and-jam -S`

The first sub-latch (the surviving conditional `br i1 %j.cmp.3, ...`)
keeps its `!dbg !14` location. The three intermediate sub-latch
positions that originally carried `!dbg !14` are now unconditional
branches with **no** `!dbg` attribute. Source-level debugging steps
"out of" iteration 0 → "into" iteration 1 of the jammed body without
any line-number information.

The same effect applies to the `AftBlocks` path (lines 547-554) and the
`CompletelyUnroll` path that replaces the final `AftTerm` (lines
534-538).

## Why this is a regression

* Bisecting source-level events (e.g., line-stepping in `gdb`/`lldb`)
  through the unrolled-and-jammed body loses the steps at the join
  points. Debuggers treat `DebugLoc()`-less terminators as "no line",
  often staying on whatever line the previous instruction had.
* Sample-based profilers (`perf`/PGO) attribute zero source line to the
  branches the program spends time on at the iteration transitions.
* `verify-debug-info` may complain in newer LLVMs that mandate non-empty
  `DebugLoc()` on terminators reachable from debug-info-bearing code.
* It is a real divergence from the sister utility `LoopUnroll.cpp`,
  which gets this right (see line 1356); the symmetry has been broken
  and no test catches it.

## Fix

Three one-liners, mirroring `LoopUnroll.cpp:1356`:

```cpp
// LoopUnrollAndJam.cpp:524 (subloop intermediate)
UncondBrInst *NewSubBr =
    UncondBrInst::Create(SubLoopBlocksFirst[It], SubTerm->getIterator());
NewSubBr->setDebugLoc(SubTerm->getDebugLoc());
SubTerm->eraseFromParent();

// LoopUnrollAndJam.cpp:537 (aft-blocks final, CompletelyUnroll)
UncondBrInst *NewAftBr =
    UncondBrInst::Create(LoopExit, AftTerm->getIterator());
NewAftBr->setDebugLoc(AftTerm->getDebugLoc());
AftTerm->eraseFromParent();

// LoopUnrollAndJam.cpp:552 (aft-blocks intermediate)
UncondBrInst *NewAftBr =
    UncondBrInst::Create(AftBlocksFirst[It], AftTerm->getIterator());
NewAftBr->setDebugLoc(AftTerm->getDebugLoc());
AftTerm->eraseFromParent();
```
