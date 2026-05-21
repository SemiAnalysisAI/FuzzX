# LoopUnroll partial unroll clones the original latch `!prof` onto every per-iteration latch

## File and root cause

`llvm/lib/Transforms/Utils/LoopUnroll.cpp` — the iteration-cloning loop
at lines 1143-1264 calls `CloneBasicBlock` for the latch block, which
copies the terminator and **all** of its metadata (including
`!prof branch_weights` and `!llvm.loop`). The post-processing code only
clears `MD_loop` from non-final latches:

```cpp
// LoopUnroll.cpp:1312-1316
// Remove loop metadata copied from the original loop latch to branches that
// are no longer latches.
for (unsigned I = 0, E = Latches.size() - (CompletelyUnroll ? 0 : 1); I < E;
     ++I)
  Latches[I]->getTerminator()->setMetadata(LLVMContext::MD_loop, nullptr);
```

`MD_prof` is left untouched. As a result, every cloned per-iteration latch
that remains a conditional branch (those whose `WillExit` returns
`std::nullopt`) ends up carrying the **same** `!prof` node copied from the
original loop. The branch weights are no longer meaningful for that
sub-iteration branch: the original weight was the per-iteration latch
exit/continue ratio of the *unrolled* loop body, but the cloned branches
now mid-body all claim the same `continue : exit` ratio as the original.

The result is that the analyses downstream (BFI, MBP) see a synthetic
profile in which "exit from sub-iteration *k* of the unrolled body" has
the same probability as the original whole-iteration exit, multiplied
along the chain — i.e. the unrolled loop appears to exit
`(exit/continue)^Count` times less frequently in cascaded estimates than
the original loop.

(There is also a separate `MD_annotation` consideration: any annotation
on the original latch terminator is also blindly duplicated to every
clone, but the more impactful issue is `MD_prof`.)

## Reproducer

`/tmp/unroll-test/test-prof-cond.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @prof_cond(ptr noalias %p, i32 %n) {
entry:
  br label %loop

loop:
  %i      = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %sum    = phi i32 [ 0, %entry ], [ %add, %loop ]
  %gep    = getelementptr i32, ptr %p, i32 %i
  %v      = load i32, ptr %gep
  %add    = add i32 %sum, %v
  %i.next = add nuw i32 %i, 1
  %cmp    = icmp ult i32 %i.next, %n
  br i1 %cmp, label %loop, label %exit, !prof !0      ; 100:1 backedge:exit

exit:
  ret i32 %add
}

!0 = !{!"branch_weights", i32 100, i32 1}
```

### `opt -passes=loop-unroll -unroll-count=4 -unroll-allow-partial -S`

```llvm
loop:
  ...
  br i1 %cmp,   label %loop.1, label %exit, !prof !0   ; iter 0 (kept clone of original)
loop.1:
  ...
  br i1 %cmp.1, label %loop.2, label %exit, !prof !0   ; iter 1 (cloned -> same MD)
loop.2:
  ...
  br i1 %cmp.2, label %loop.3, label %exit, !prof !0   ; iter 2 (cloned -> same MD)
loop.3:
  ...
  br i1 %cmp.3, label %loop,   label %exit, !prof !0, !llvm.loop !1   ; iter 3 / backedge
```

All four latch branches carry the *identical* `!prof !0 = {100, 1}`. There
is no per-iteration adjustment, and no merging with what the comment at
line 1312 calls out for `MD_loop` removal.

If a profile-aware analysis reads this verbatim, it will compute the
expected exit count along the chain:

```
P(exit at iter k | reached iter k) = 1/101
P(reach iter k+1 | reached iter k)  = 100/101
P(reach iter 3)                     = (100/101)^3 = 0.9707
P(reach exit via iter 3 backedge)   = (100/101)^4 = 0.961
```

That is the exit probability *per unrolled body execution* — but the
original program had `P(exit per iteration) = 1/101` so the unrolled body
"per iteration" probability (counting one unrolled body as four source
iterations) should be `1 - (100/101)^4 = 0.0388`. The propagated MD
expresses this only through composition of the four identical branches,
which is correct *only* if the consumer composes them and ignores the
fact that all four are reported as the same per-branch probability. Many
consumers (`BranchProbabilityInfo`, `BlockFrequencyInfo`) compose
incorrectly here because they treat the four branches as independent
sequential branches, accumulating an exit-rate four times higher than the
original.

The narrower bug, regardless of accumulation semantics, is that the
original `!prof` is propagated *unmodified* onto branches it was never
attached to. The comment at line 1312 calls out this kind of stale-MD
cleanup for `MD_loop`; the analogous cleanup for `MD_prof` (clear, or
recompute by `(orig_continue)^N : 1 - (orig_continue)^N` for the backedge,
and `(orig_continue)^(N-k-1) * orig_exit : ...` for intermediate exits)
is absent.

## Compare full unroll

For full unroll (trip count known, `CompletelyUnroll == true`) the latch
branches are folded to unconditional branches by `SetDest`, which drops the
MD entirely (UncondBr doesn't carry `!prof`). So the bug is unique to
*partial* unroll where the per-iteration latches remain conditional.

## Why this is a regression

* Block-frequency estimates for the unrolled body and any code reachable
  from the early-exit successors of intermediate iterations are off — the
  exit edge from each iteration is reported with the original
  per-iteration probability, which inflates the expected cumulative exit
  rate by ~`Count`x in naive composition.
* For PGO-guided downstream passes this can mis-place blocks
  (`MachineBlockPlacement`), mis-bias inlining decisions, or mis-size
  cold/hot partitions for `-fdata-sections`.
* For LoopVectorize/IfConvert running after, the misleading exit
  probability of intermediate sub-iterations can make those branches look
  poorly predictable when in fact they are deterministic by IV
  arithmetic.

## Fix sketch

After the `MD_loop` cleanup loop at line 1314-1316, add:

```cpp
// Scale the latch !prof so that the *cumulative* exit rate across the
// `Count` sub-iterations matches the original per-iteration exit rate.
if (MDNode *MD = LatchBlock->getTerminator()->getMetadata(LLVMContext::MD_prof)) {
  // Extract original (Backedge, Exit) weights.
  uint64_t Be, Ex;
  if (extractBranchWeights(*MD, Be, Ex)) {
    // Per cloned-iter exit weight: Ex / Count (rounded).
    // Backedge weight remains Be on the final clone (the unrolled-loop
    // backedge), but intermediate cloned latches must reflect a "we did
    // not exit this sub-iter" probability conditional on reaching it,
    // which is (Be) : (Ex) only on the *original* clone (iteration 0),
    // and a derived value on intermediate clones.
    // ...
  } else {
    // Failed to parse: clear MD on intermediate clones, keep on the
    // backedge clone.
    for (unsigned I = 0, E = Latches.size() - 1; I < E; ++I)
      Latches[I]->getTerminator()->setMetadata(LLVMContext::MD_prof, nullptr);
  }
}
```

At minimum, clearing `MD_prof` from intermediate cloned latches (so they
are treated as unbiased rather than as carrying the original per-iter
ratio) is strictly better than the current silent verbatim duplication.
