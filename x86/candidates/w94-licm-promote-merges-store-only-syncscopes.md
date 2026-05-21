# LICM promoteLoopAccessesToScalars merges store-only mismatched syncscopes

## Severity
Memory-model miscompile. Distinct manifestation of the same root cause as
w86-licm-promote-merges-mismatched-syncscopes.md (#144), but in a code shape
that contains only stores (no load on the location in the loop body). The
resulting code has *one* exit-block store at the default System scope, which
is the sink of values that originally came from multiple unordered atomic
stores with *different* SyncScopeIDs. The store that used to be
`syncscope("singlethread")` (no cross-thread synchronization contract) is
silently promoted to `syncscope("system")` (cross-thread visible) -- the
dangerous direction for a writer in a signal handler or compiler-runtime
fence-elision idiom that relies on "this store does not synchronize with
other threads".

This case is materially distinct from #144 because:
- #144's repro has one load and one store on the location, both unordered
  atomic with different scopes. The pass emits both a preheader load and an
  exit-block store, both at System.
- This repro has *two stores* on the location, no load. The pass emits *only*
  one exit-block store at System -- there is no preheader load. The
  load-side guard would not catch this; only store-side guards do.

## Source
`llvm/lib/Transforms/Scalar/LICM.cpp`, `promoteLoopAccessesToScalars()`.
- Use-walk over `PointerMustAliases` (~lines 2025-2122) checks
  `Store->isUnordered()` only; SyncScopeID is never compared.
- `LoopPromoter::insertStoresInLoopExitBlocks()` (~line 1825) calls
  `setOrdering(AtomicOrdering::Unordered)` only. SyncScope on the inserted
  StoreInst is left at the default `SyncScope::System`.

## Repro
```ll
; /tmp/w94/store_only_mismatch.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

@G = global i32 0, align 4

define i32 @store_only(i32 %n) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  ; First store: syncscope("singlethread") -- intra-thread only contract
  store atomic i32 %i, ptr @G syncscope("singlethread") unordered, align 4
  ; Second store: default (System) scope
  store atomic i32 %i, ptr @G unordered, align 4
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit:
  ret i32 0
}
```

## opt diff
```
$ opt -passes='loop-mssa(licm)' -S store_only_mismatch.ll
...
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit

exit:
  %i.lcssa = phi i32 [ %i, %loop ]
  store atomic i32 %i.lcssa, ptr @G unordered, align 4   ; <-- ONE store at System scope
  ret i32 0
```

Both original stores have been deleted; only one exit store remains, at
default System scope. The `syncscope("singlethread")` semantic of the first
original store is silently broadened to `syncscope("system")`.

## Why this is dangerous
A library that performs `syncscope("singlethread")` stores to communicate
with a same-thread signal handler is now emitting a System-scope store, which
on weakly-ordered targets (AArch64, AMDGPU, RISC-V) lowers to *different*
machine instructions with different cost / different fences. The promoted IR
no longer satisfies the source intent and the assembler may now (1) be
slower than promised, (2) require fences elsewhere that the user didn't add,
(3) prevent some target backends from elision optimizations that singlethread
permits. Reverse direction (System -> singlethread) would actually drop the
cross-thread synchronization contract entirely, allowing reordering across
threads that the original IR forbade.

## Fix sketch
Same as #144: track SyncScopeID during the use-walk and bail (or refuse to
add the conflicting access to the promotion set) if scopes disagree. Plumb
the consensus SSID into `LoopPromoter::insertStoresInLoopExitBlocks()` and
call `NewSI->setSyncScopeID(SSID)`. A regression test for this shape
(store-only, no load) should accompany the fix because the existing #144
LIT-style tests use the load+store shape.

## Verified with
- opt commit: `LLVM version 23.0.0git` from
  `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt`
- Run: `opt -passes='loop-mssa(licm)' -S store_only_mismatch.ll`
