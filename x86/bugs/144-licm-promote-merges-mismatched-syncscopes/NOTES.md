# LICM promoteLoopAccessesToScalars merges atomic accesses with mismatched syncscopes

## Severity
Memory-model miscompile. When two atomic accesses to the same location in a
loop carry *different* SyncScopeIDs (e.g. one `syncscope("singlethread")` load
and one default-scope store), `promoteLoopAccessesToScalars` does not detect
the mismatch and produces a single preheader load + exit-block store, both
emitted at the default System SyncScope. The original loop's
singlethread-scoped access is silently promoted to System scope (changes
synchronizes-with relations with other threads).

## Source
`llvm/lib/Transforms/Scalar/LICM.cpp`, `promoteLoopAccessesToScalars()`.

Around line 1999 we track:
```cpp
bool SawUnorderedAtomic = false;
bool SawNotAtomic = false;
```
but there is no analogous tracking for SyncScopeID. The use-walk on
`PointerMustAliases` (~lines 2025-2122) accepts any unordered atomic load or
store regardless of SyncScopeID and only bails on the
`SawUnorderedAtomic && SawNotAtomic` mix (~line 2129).

Then `LoopPromoter` (~line 1799) and the preheader load (~line 2200) call
`setOrdering(AtomicOrdering::Unordered)` without ever setting
`setSyncScopeID(...)`. So all inserted atomic ops default to
`SyncScope::System`.

## Repro
```ll
; /tmp/w86/ss_mismatch.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

@G = global i32 0, align 4

define i32 @ss_mismatch(i32 %n) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %sum = phi i32 [ 0, %entry ], [ %sum.next, %loop ]
  ; load with syncscope("singlethread")
  %v = load atomic i32, ptr @G syncscope("singlethread") unordered, align 4
  %sum.next = add i32 %sum, %v
  ; store with default (System) syncscope
  store atomic i32 %sum.next, ptr @G unordered, align 4
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit:
  ret i32 %sum.next
}
```

## opt diff
```
$ opt -passes='loop-mssa(licm)' -S ss_mismatch.ll
...
entry:
  %G.promoted = load atomic i32, ptr @G unordered, align 4           ; <-- syncscope("singlethread") of original load LOST
  br label %loop
loop:
  %sum.next1 = phi i32 [ %G.promoted, %entry ], [ %sum.next, %loop ]
  ...
  %sum.next = add i32 %sum, %sum.next1
  ...
exit:
  %sum.next.lcssa2 = phi i32 [ %sum.next, %loop ]
  ...
  store atomic i32 %sum.next.lcssa2, ptr @G unordered, align 4       ; <-- store unchanged scope (was System anyway)
  ret i32 ...
```
The promoted load was originally `syncscope("singlethread")`, now it is
`syncscope("system")` (default). A signal handler or compiler-runtime that
relies on the singlethread-only ordering with the rest of the same thread
will see different reordering behavior after this transformation.

The symmetric direction is just as bad: if the load had been default scope
and the store had been `syncscope("singlethread")`, promotion would emit a
system-scope store unaffectedly, but again the original code's
singlethread-only contract is laundered into a system-scope contract,
needlessly forcing the store onto the cross-thread ordering.

## Fix sketch
1. In the use-walk in `promoteLoopAccessesToScalars`, track the
   `SyncScopeID` of each unordered atomic load/store. If any two accesses
   disagree, return false (bail on promotion).
2. Plumb the consensus SyncScopeID through `LoopPromoter` (next to
   `UnorderedAtomic`) and call `setSyncScopeID(SSID)` on the preheader
   `LoadInst` and each exit-block `StoreInst`.
3. Existing test `llvm/test/Transforms/LICM/atomics.ll` covers default-scope
   atomics only; add coverage for `syncscope("singlethread")`.

## Note: shared root cause with w86-licm-promote-drops-syncscope.md
That candidate documents the simpler case of a single SyncScope being
dropped. This candidate is the same underlying defect surfaced as an
unsoundness when the loop's accesses have mismatched scopes -- the pass
silently picks one (System) and discards the rest.
