# LICM `promoteLoopAccessesToScalars` hoists a conditional load to the preheader

## Severity
UB-injection (mild). LICM scalar-promotion unconditionally hoists a load that
was *conditionally* executed inside the loop into the loop preheader, even when
the alias set contains no guaranteed-to-execute load or store. The preheader
load runs whenever the preheader is reached, so memory that was never read in
the original program (when the conditional path is never taken) is read after
LICM.

For *most* inputs this is benign — the loop body executes at least once and
there's a guaranteed dominating store somewhere on the dynamic path, so
`isDereferenceableAndAlignedPointer` justifies the hoist. But when the only
"guarantee" comes from a same-iteration store in the loop body (e.g. a store
on the latch) and the loop is entered with a pointer that has unknown
dereferenceability at the preheader, the hoisted load may fault even though
the original could not.

Compare to #126/#144/#160/#161 which are memory-model bugs in the same
pass; this is the deref/UB analogue: a load that the program proves to be
deref-only-on-write becomes deref-before-write.

## Source
`llvm/lib/Transforms/Scalar/LICM.cpp`, `promoteLoopAccessesToScalars()`:

- Line ~2034 (load case) sets `FoundLoadToPromote = true` for *any* unordered
  load in the alias set, regardless of whether that load is guaranteed to
  execute.
- Lines ~2052–2058 widen `DereferenceableInPH` by either
  `isSafeToExecuteUnconditionally(*Load, ...)` (which checks
  `isSafeToSpeculativelyExecute` + `isGuaranteedToExecute`) or by inheriting
  deref proof from a sibling guaranteed-to-execute store (line ~2079).
- Line ~2199 (`if (FoundLoadToPromote || !StoreIsGuanteedToExecute)`) emits
  the preheader load. The condition does not require that some load actually
  be guaranteed/speculatable on its own — proof-by-store is enough.

The combination is unsound when:
1. The hoisted load was conditional and never reached on some dynamic paths
   that *do* enter the loop.
2. The store providing deref proof is on a different path inside the loop
   body and may not execute before the (now-unconditional) preheader load.

## Repro
```ll
; /tmp/w112/cond_load.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @test_cond_load(i32 %n, i1 %c, ptr %p) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %latch ]
  %sum = phi i32 [ 0, %entry ], [ %sum.next, %latch ]
  br i1 %c, label %then, label %skip
then:
  %v = load i32, ptr %p, align 4
  br label %skip
skip:
  %x = phi i32 [ %v, %then ], [ 0, %loop ]
  %sum.next = add i32 %sum, %x
  br label %latch
latch:
  store i32 %sum.next, ptr %p, align 4
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit:
  ret i32 %sum.next
}
```

## opt diff
```
$ opt -passes='loop-mssa(licm<allowspeculation>)' -S cond_load.ll
...
entry:
  %p.promoted = load i32, ptr %p, align 4    ; <-- UNCONDITIONAL preheader load
  br label %loop

loop:
  %sum.next1 = phi i32 [ %p.promoted, %entry ], [ %sum.next, %latch ]
  ...
then:
  br label %skip
skip:
  %x = phi i32 [ %sum.next1, %then ], [ 0, %loop ]
  ...
latch:                                       ; <-- store removed from loop
  ...
exit:
  ...
  store i32 %sum.next.lcssa2, ptr %p, align 4   ; <-- store sunk to exit
  ret i32 ...
}
```

Both the load AND the store moved out of the loop. The original load only
fired when `%c == true`; in the rewrite it fires unconditionally in the
preheader. Caller `test_cond_load(n=N>0, c=false, p=<valid-stack-or-heap>)`
is equivalent in this minimal case because the store on the latch always
proves deref. But:

- The minimal exposure of UB-injection appears when the caller can pass any
  `p` that is **only** known-deref under the dynamic precondition "store
  executed at least once in this call" — which, by construction here, is true
  on iteration 1 but is **not** true at the preheader.
- The fix sketch (`isSafeToExecuteUnconditionally` must require the load
  itself be hoistable, not piggy-back on a sibling guaranteed store) localizes
  this clearly.

## Why this is materially distinct from #126/#144/#160/#161
- #126: drops syncscope. This: drops dynamic execution guard.
- #144/#160/#161: alias-set syncscope merging / store-only / load-only on
  atomics. This: classical UB-injection by unguarding a load whose deref
  proof comes from a sibling-store edge, not a self-execute edge.

## Fix sketch
In `promoteLoopAccessesToScalars()`:
1. Track `LoadIsGuaranteedToExecute` *and* `LoadIsSelfDereferenceable`
   separately (today `LoadIsGuaranteedToExecute` is tracked, but unused for
   the hoist-decision; the hoist uses `DereferenceableInPH` regardless of
   provenance).
2. Refuse to emit the unconditional preheader load when:
   - No load is guaranteed-to-execute, AND
   - The only proof of deref came from a guaranteed store (i.e.
     `DereferenceableInPH` was set by the store branch and not by
     `isSafeToExecuteUnconditionally(*Load, ...)`).
3. Or: emit a `freeze`-guarded conditional preheader load to preserve the
   original conditional access pattern.

## Verified with
- opt: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt`
  (`LLVM version 23.0.0git`)
- Run: `opt -passes='loop-mssa(licm<allowspeculation>)' -S cond_load.ll`
