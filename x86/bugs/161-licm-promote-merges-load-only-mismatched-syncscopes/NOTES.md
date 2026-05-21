# LICM promoteLoopAccessesToScalars merges load-only mismatched syncscopes

## Severity
Memory-model miscompile. Same underlying root cause as #144
(`w86-licm-promote-merges-mismatched-syncscopes.md`), separate manifestation:
when the loop contains two atomic *loads* of the same location with *different*
SyncScopeIDs (e.g. one `syncscope("singlethread")` and one default-scope),
plus a same-scope store, `promoteLoopAccessesToScalars` quietly chooses one
SyncScope (System / default) for the preheader load and another (System) for
the exit store, throwing away the per-load scope information.

This is distinct from #144 because the conflict is between two *loads* on the
same location, not between a load and a store. The promotion logic only sees
"two unordered atomic loads, both fit; one unordered atomic store, fits" and
proceeds happily. Only the store-side guards exist; the load-side does not
compare SyncScopeID.

## Source
`llvm/lib/Transforms/Scalar/LICM.cpp`, `promoteLoopAccessesToScalars()`
(~line 2034 for the Load case):
```cpp
if (LoadInst *Load = dyn_cast<LoadInst>(UI)) {
  if (!Load->isUnordered())
    return false;
  SawUnorderedAtomic |= Load->isAtomic();
  SawNotAtomic |= !Load->isAtomic();
  ...
```
The Load case looks only at the `unordered`/`atomic` bits and at alignment.
No `getSyncScopeID()` query. Same for the Store case (~line 2059). The
preheader load is then constructed (~line 2200) with only
`setOrdering(AtomicOrdering::Unordered)` -- default System SyncScope.

## Repro
```ll
; /tmp/w94/load_only_mismatch.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

@G = global i32 0, align 4

define i32 @load_only(i32 %n) {
entry:
  br label %loop
loop:
  %i  = phi i32 [ 0, %entry ], [ %i.next, %latch ]
  %sum = phi i32 [ 0, %entry ], [ %sum.next, %latch ]
  %even = and i32 %i, 1
  %tobit = icmp eq i32 %even, 0
  br i1 %tobit, label %even.bb, label %odd.bb
even.bb:
  %v1 = load atomic i32, ptr @G syncscope("singlethread") unordered, align 4
  br label %latch
odd.bb:
  %v2 = load atomic i32, ptr @G unordered, align 4
  br label %latch
latch:
  %v = phi i32 [%v1, %even.bb], [%v2, %odd.bb]
  store atomic i32 %v, ptr @G syncscope("singlethread") unordered, align 4
  %sum.next = add i32 %sum, %v
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit:
  ret i32 %sum.next
}
```

## opt diff
```
$ opt -passes='loop-mssa(licm)' -S load_only_mismatch.ll
...
entry:
  %G.promoted = load atomic i32, ptr @G unordered, align 4         ; <-- syncscope LOST (was singlethread or system, now system)
  br label %loop

loop:
  %v3 = phi i32 [ %G.promoted, %entry ], [ %v, %latch ]
  ...
latch:
  %v = phi i32 [ %v3, %even.bb ], [ %v3, %odd.bb ]
  ...
exit:
  ...
  store atomic i32 %v.lcssa, ptr @G unordered, align 4            ; <-- syncscope("singlethread") LOST on the original store
  ret ...
```

Both directions are present:
1. Preheader load is `system`; original `%v1` was `syncscope("singlethread")`.
2. Sunk exit store is `system`; original store was `syncscope("singlethread")`.

## Why this is materially distinct from #144
- #144: load with scope A + store with scope B in the loop body.
- this candidate: two loads with scopes A and B + one store with scope A.

The use-walk traversal hits two LoadInsts of mismatched scope on the same
PointerMustAlias entry. A correct implementation must detect that and either
bail or emit a single consensus scope. The existing code accepts both into
`SawUnorderedAtomic` indiscriminately.

A correct fix has to plumb scope tracking through both branches of the walk
(LoadInst case AND StoreInst case), not just one of them. A LIT test that
only covers the load+store mismatch (i.e., a fix that addresses #144 by
checking only one branch) will still let this load+load case slip through.

## Fix sketch
1. Add `SyncScope::ID FirstSSID = SyncScope::SingleThread`-sentinel and
   `bool SawAtomic = false` at the top of the use-walk.
2. In *both* the Load and the Store branches, on the first atomic access,
   record `getSyncScopeID()`. On every subsequent atomic access, compare
   against the recorded value and `return false` on mismatch.
3. Plumb the recorded SSID into `LoopPromoter` and apply it on both the
   preheader load and the exit-block stores.

## Verified with
- opt commit: `LLVM version 23.0.0git` from
  `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt`
- Run: `opt -passes='loop-mssa(licm)' -S load_only_mismatch.ll`
