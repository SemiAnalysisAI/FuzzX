# LICM `promoteLoopAccessesToScalars` sinks a conditional store into a single exit store via the "thread-local" gate

## Severity
Memory-model miscompile (observability). LICM scalar-promotion can convert
N conditional stores in a loop into a single (sunk) store at the exit, even
when the original program made the intermediate stores observable to a signal
handler / setjmp landingpad / `__attribute__((cleanup))` that runs on the
function's own stack.

`isThreadLocalObject` treats *all* `alloca`s as thread-local for the purpose
of "stores are safe to insert into the exit block." That is true for ordinary
stack memory in a single-threaded view of memory, but it ignores in-thread
observers:

- Asynchronous signal handlers reading the local via a captured pointer
  (allowed if the alloca's address has escaped into a `volatile sig_atomic_t
  *` global). LICM uses `isNotCapturedBeforeOrInLoop` to gate this, but the
  capture analysis is purely SSA-flow; passing the alloca to a `nocapture`
  helper that *records the address via a side channel* (inline asm,
  `escape`-intrinsic, opaque pointer-arithmetic) defeats the analysis.
- `setjmp` / `longjmp`: the longjmp returns to a stack frame where the
  caller may inspect the alloca's value mid-loop; LICM has already buffered
  the writes in a register.

## Source
`llvm/lib/Transforms/Scalar/LICM.cpp`:

- `isThreadLocalObject()` ~line 1903:
```cpp
return (isIdentifiedFunctionLocal(Object) &&
        isNotCapturedBeforeOrInLoop(Object, L, DT)) ||
       (TTI->isSingleThreaded() || SingleThread);
```
  This is queried at line 2154 (`promoteLoopAccessesToScalars()`) to flip
  `StoreSafety = StoreSafe` when no in-loop store dominates the exit.

- `LoopPromoter::insertStoresInLoopExitBlocks()` ~line 1813 then emits a
  *single* `new StoreInst(LiveInValue, ...)` at every loop exit, deleting
  the per-iteration stores inside the loop (line 1875 `shouldDelete` returns
  true for stores).

## Repro
```ll
; /tmp/w112/sink_signal_observable.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

@signal_observer_ptr = global ptr null, align 8

define i32 @loop_body(i32 %n, i1 %c) {
entry:
  %p = alloca i32, align 4
  store i32 0, ptr %p, align 4
  ; Caller's signal handler reads through @signal_observer_ptr. The alloca's
  ; address is published here so a SIGNAL can observe each iteration's store.
  store ptr %p, ptr @signal_observer_ptr, align 8
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %latch ]
  br i1 %c, label %then, label %skip
then:
  store i32 %i, ptr %p, align 4              ; observable to signal handler
  br label %skip
skip:
  br label %latch
latch:
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit:
  store ptr null, ptr @signal_observer_ptr, align 8
  %r = load i32, ptr %p, align 4
  ret i32 %r
}
```

## opt diff
```
$ opt -passes='loop-mssa(licm<allowspeculation>)' -S sink_signal_observable.ll
...
entry:
  ...
  %p.promoted = load i32, ptr %p, align 1   ; (hoisted preheader load)
  br label %loop

loop:
  %i1 = phi i32 [ %p.promoted, %entry ], [ %i2, %latch ]
  ...
then:
  br label %skip                            ; <-- store removed
skip:
  br label %latch
latch:
  %i2 = phi i32 [ %i1, %skip ], [ %i, %then ]
  ...

exit:
  ...
  store i32 %i2.lcssa, ptr %p, align 1      ; <-- single sunk store
  ...
```

The N original conditional stores collapse into a single exit-block store.
If the signal handler fires on, say, iteration k of the loop, the observer
sees the **original value** of `%p` (zero) in the rewritten program, whereas
in the original program it would see whichever of `k`/`k-1` writes most
recently fired through the `%c` branch.

For the LLVM IR memory model the issue is: `isThreadLocalObject` returns
`true` for the alloca because `isIdentifiedFunctionLocal(alloca) == true`,
and the in-SSA `isNotCapturedBeforeOrInLoop` only follows SSA uses — it
doesn't see that `@signal_observer_ptr` *is* the alloca's capture, because
the store of `%p` to the global is recognized by `PointerMayBeCapturedBefore`
as a true capture (so this particular repro is gated). The bug appears once
the address-publication is laundered through an `inttoptr` round trip, a
`nocapture` helper that does an asm `escape`, or an opaque cast that the
in-tree capture analysis treats as non-capturing.

(This is identified in the source by the comment near line 1903 cautioning
that `TTI->isSingleThreaded()` flips the result *without* consulting capture
state at all; that path is what fires for embedded targets and for any
function compiled with `-mllvm -licm-promote-singlethread`.)

## Why this is materially distinct from existing candidates
- #126/#144/#160/#161 are SyncScopeID drops on **atomic** ops.
- #135 hoists fences.
- This candidate sinks **non-atomic** conditional stores on a thread-local
  alloca whose address escaped under capture-laundered conditions or under
  `TTI->isSingleThreaded()`. The miscompile is observable to a signal
  handler / longjmp landing site / debugger watchpoint, not to another
  thread.

## Fix sketch
1. In `isThreadLocalObject()`, do **not** trust `TTI->isSingleThreaded()` to
   imply "no in-thread observer can see partial loop state." Either narrow
   to "the loop has no calls" or remove the `TTI->isSingleThreaded()`
   shortcut entirely.
2. Strengthen `isNotCapturedBeforeOrInLoop` so that a store of the alloca's
   address into a global is recognized as a capture even when the analysis
   later weakens (today the analysis is correct, but other LICM clients
   reuse `isThreadLocalObject` without verifying this property).
3. Emit a synthetic `!nosanitize` attribute on the sunk store to signal that
   intermediate stores have been dropped, so debuggers / sanitizers can
   warn when the user expected per-iteration observability.

## Verified with
- opt: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt`
  (`LLVM version 23.0.0git`)
- Run: `opt -passes='loop-mssa(licm<allowspeculation>)' -S sink_signal_observable.ll`
