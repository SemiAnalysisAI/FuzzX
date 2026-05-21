# DSE partial-store merging drops `!annotation` from the killing store

**Component:** llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp

**Function:** `DSEState::eliminateDeadDefs(MemoryLocationWrapper&)` — partial-merge branch

**Lines:** 2683-2708 (the `EnablePartialStoreMerging && OR == OW_PartialEarlierWithFullLater`
arm). Mutation at 2695 (`DeadSI->setOperand(0, Merged);`); deletion at 2702
(`deleteDeadInstruction(KillingSI, &Deleted);`).

## Pattern

When an earlier "dead" store is partially overwritten by a later "killing"
store, and `tryToMergePartialOverlappingStores` succeeds, DSE:

1. Mutates the dead store's value operand to a merged constant (line 2695).
2. Deletes the killing store (line 2702).

Nothing in this branch transfers metadata from the killing store to the
surviving dead store. In particular, `!annotation` MDs (used by sample-PGO
and various source-attribution tooling) attached to the killing store are
silently lost.

## Bug

The killing `StoreInst` may carry `!annotation` (or `!prof`, `!nontemporal`,
`!noalias`, etc.). When the merge fires, the killing store disappears and
its metadata goes with it. The dead store is left with the merged value but
keeps only its own original metadata (which may be unrelated or absent).

There is no analogue of `combineMetadata` / `combineMetadataForCSE` in this
branch — unlike CSE-style merges that explicitly combine metadata, the DSE
partial-merge just deletes the killing store outright.

## Confirmed via `opt -passes=dse` (x86_64, default `-O2`)

### Input IR
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }
declare void @use(ptr)

define void @f(ptr %p) {
  %a = alloca %S, align 8
  store i64 0, ptr %a, align 8                              ; "dead" — partially overwritten
  store i32 7, ptr %a, align 4, !annotation !0              ; "killing" — full lane overwrite
  call void @use(ptr %a)
  ret void
}

!0 = !{!"important"}
```

### After `opt -passes=dse -S`
```
define void @f(ptr %p) {
  %a = alloca %S, align 8
  store i64 7, ptr %a, align 8                              ; merged: 0 | (7<<0)
  call void @use(ptr %a)
  ret void
}
```

The surviving `store i64 7` carries **no** `!annotation`. The metadata
attached to the killing `store i32 7, ..., !annotation !0` was dropped on
deletion at line 2702 with no attempt to combine it onto the surviving
`DeadSI` mutated at line 2695.

Also reachable via the default x86 `-O2` pipeline (`opt -O2 -S` produces
the same merged store with no annotation).

## Fix sketch

Before `deleteDeadInstruction(KillingSI, ...)` at line 2702, combine
metadata from `KillingSI` onto `DeadSI` for the metadata kinds that survive
a move (in particular `MD_annotation` should be union-merged per its
existing semantics; `MD_noalias`/`MD_alias_scope` should be intersected).
The existing helper `combineMetadataForCSE(DeadSI, KillingSI, /*DoesKMove=*/true)`
is the standard idiom.
