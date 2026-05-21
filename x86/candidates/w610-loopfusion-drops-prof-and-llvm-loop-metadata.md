# w610: LoopFusion drops `!prof` and `!llvm.loop` from the first loop's latch

- **Pass**: `llvm/lib/Transforms/Scalar/LoopFuse.cpp` (LoopFusePass)
- **Gating**: NOT in default `-O2`. Requires opt flag `--enable-loopfusion`
  (default `LoopFusion = false` at `llvm/lib/Passes/PassBuilderPipelines.cpp:337`,
  enabled by `llvm/tools/opt/NewPMDriver.cpp:71` `EnableLoopFusion`).
  Triggers in optimization pipeline at `PassBuilderPipelines.cpp:1601-1602`.
- **Severity**: Loss of correctness-affecting metadata (`!prof` branch weights
  and `!llvm.loop` directives) on every successful fusion. Optimizer subsequent
  loop transformations and PGO/BFI all see only FC1's profile/loop directives.

## Root cause

`LoopFuser::simplifyLatchBranch` (LoopFuse.cpp:1321-1330) rewrites the FC0
latch terminator to an unconditional branch via `ReplaceInstWithInst`:

```cpp
void simplifyLatchBranch(const FusionCandidate &FC) const {
  CondBrInst *FCLatchBranch = dyn_cast<CondBrInst>(FC.Latch->getTerminator());
  if (FCLatchBranch) {
    assert(FCLatchBranch->getSuccessor(0) == FCLatchBranch->getSuccessor(1) &&
           "Expecting the two successors of FCLatchBranch to be the same");
    UncondBrInst *NewBranch =
        UncondBrInst::Create(FCLatchBranch->getSuccessor(0));
    ReplaceInstWithInst(FCLatchBranch, NewBranch);  // <-- drops all metadata
  }
}
```

`ReplaceInstWithInst` (`llvm/lib/Transforms/Utils/BasicBlockUtils.cpp:624-642`)
only copies the `!dbg` (DebugLoc) — `!prof` and `!llvm.loop` on the original
conditional branch are *not* carried over.

In `performFusion` (LoopFuse.cpp:1492-1498), after FC0's latch terminator is
re-pointed to FC1.Header and FC1's latch terminator is re-pointed to
FC0.Header, both successors of FC0's latch branch are the same so
`simplifyLatchBranch(FC0)` is invoked. FC0's `!llvm.loop` (containing e.g.
`mustprogress`, distribution/unroll hints) and `!prof` weights are silently
dropped at this point. The fused loop's loop ID is inherited solely from FC1's
latch terminator (the surviving back-edge brings FC1's `!llvm.loop`).

## Reproducer

`/tmp/w610/fuse2.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @fuse_meta(ptr noalias %A, ptr noalias %B) {
entry:
  br label %loop1

loop1:
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop1 ]
  %gepA = getelementptr i32, ptr %A, i64 %i
  store i32 1, ptr %gepA, align 4
  %i.next = add nuw nsw i64 %i, 1
  %c1 = icmp ult i64 %i.next, 100
  br i1 %c1, label %loop1, label %loop1.exit, !prof !10, !llvm.loop !20

loop1.exit:
  br label %loop2

loop2:
  %j = phi i64 [ 0, %loop1.exit ], [ %j.next, %loop2 ]
  %gepB = getelementptr i32, ptr %B, i64 %j
  store i32 2, ptr %gepB, align 4
  %j.next = add nuw nsw i64 %j, 1
  %c2 = icmp ult i64 %j.next, 100
  br i1 %c2, label %loop2, label %exit, !prof !11, !llvm.loop !21

exit:
  ret void
}

!10 = !{!"branch_weights", i32 100, i32 1}
!11 = !{!"branch_weights", i32 200, i32 1}
!20 = distinct !{!20, !22}
!21 = distinct !{!21, !23}
!22 = !{!"llvm.loop.mustprogress"}
!23 = !{!"llvm.loop.unroll.disable"}
```

Command:

```bash
opt -S -passes='loop-fusion' --enable-loopfusion /tmp/w610/fuse2.ll
```

## Output diff (key portion)

Before fusion the loops carry distinct metadata:

```
loop1 latch:  br ... !prof !10 (100,1),   !llvm.loop !20 (mustprogress)
loop2 latch:  br ... !prof !11 (200,1),   !llvm.loop !21 (unroll.disable)
```

After fusion (single combined loop):

```
loop1:                                            ; preds = %loop1, %entry
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop1 ]
  %j = phi i64 [ 0, %entry ], [ %j.next, %loop1 ]
  ...
  br i1 %c2, label %loop1, label %exit, !prof !0, !llvm.loop !1

!0 = !{!"branch_weights", i32 200, i32 1}          ; ONLY FC1's weights survive
!1 = distinct !{!1, !2}
!2 = !{!"llvm.loop.unroll.disable"}                ; ONLY FC1's loop ID survives
```

FC0's `!prof !10 = {100, 1}` and FC0's `!llvm.loop !20 / !22 (mustprogress)`
are nowhere in the output IR.

## Impact

1. **PGO loss**: BFI / profile-guided heuristics see only FC1's trip estimate
   for the merged loop. If FC0 had a more accurate weight (e.g. cold loop
   fused into hot loop or vice versa) downstream block frequency is wrong.
2. **Loop directive loss**: `llvm.loop.mustprogress`, `llvm.loop.unroll.*`,
   `llvm.loop.vectorize.*`, `llvm.loop.distribute.*`, `parallel_accesses`,
   etc. from FC0 silently disappear. Loop semantics that rely on
   `mustprogress` (e.g. forward-progress guarantees) can be invalidated; unroll
   hints may be ignored; user-pragma intent from FC0 is lost.
3. There is no merge of the two loop IDs — the fused loop should at minimum
   carry the union (or intersection) of compatible loop metadata, not blindly
   inherit one side's.

## Suggested fix sketch

In `simplifyLatchBranch`, preserve metadata:

```cpp
UncondBrInst *NewBranch = UncondBrInst::Create(FCLatchBranch->getSuccessor(0));
NewBranch->copyMetadata(*FCLatchBranch);   // !prof, !llvm.loop, !make.implicit, ...
ReplaceInstWithInst(FCLatchBranch, NewBranch);
```

Then in `performFusion`, after the FC0/FC1 latches are rewired, merge the loop
IDs (e.g. with `MDNode::concatenate` filtering for known mergable directives)
and `setLoopID` on `FC0.L` (the surviving loop); also merge `!prof` on the
remaining conditional back-edge (FC1.Latch's terminator) with FC0's old `!prof`
(typically taking the maximum of true counts, or a weighted average).
