# NewGVN `StoreExpression` CSE drops `!nontemporal` metadata

## File and root cause

`llvm/lib/Transforms/Scalar/NewGVN.cpp` —
`NewGVN::performSymbolicStoreEvaluation` (line 1424) and `StoreExpression::equals`
(line 931).

When two `store` instructions write the same value to the same address with the
same MemorySSA defining access, `performSymbolicStoreEvaluation` puts the
second store into the same `CongruenceClass` as the first via the
`LastStore`/`LastCC` lookup at lines 1445-1453. The later store is then
marked for deletion in `eliminateInstructions`, and uses pointing at it are
RAUW'd to the leader store.

`StoreExpression::equals` only checks `equalsLoadStoreHelper` plus the stored
value. It does NOT consider `!nontemporal` metadata. Together with
`combineMetadataForCSE`'s rule that `!nontemporal` is preserved on the kept
instruction only "if it is present on both instructions" (`Local.cpp:3030`),
this means the `!nontemporal` hint is dropped from the surviving store whenever
the eliminated store was the one carrying it.

## Reproducer

`x86/candidates/w99-store-nontemporal-r.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @test(ptr %p, i32 %v) {
entry:
  store i32 %v, ptr %p, align 4
  store i32 %v, ptr %p, align 4, !nontemporal !0
  ret void
}

!0 = !{i32 1}
```

### `opt -passes=newgvn` diff

Before:
```llvm
  store i32 %v, ptr %p, align 4
  store i32 %v, ptr %p, align 4, !nontemporal !0
```

After:
```llvm
  store i32 %v, ptr %p, align 4
```

The second store (the one with `!nontemporal`) is removed, and the surviving
first store does NOT receive the `!nontemporal` metadata.

For comparison:
* Regular `-passes=gvn` keeps BOTH stores (no CSE between them).
* `-passes=dse` correctly removes the FIRST store and keeps the `!nontemporal`
  one — the opposite of what NewGVN does.

## Why this is a regression

LangRef says `!nontemporal` "tells the optimizer and code generator that this
[store] is not expected to be reused in the cache. The code generator may
select special instructions to save cache bandwidth, such as the `MOVNT`
instruction on x86." Dropping `!nontemporal` causes:

1. Loss of a programmer-visible perf intent (e.g., streaming stores).
2. On x86, codegen emits `MOV`/`vmovaps` instead of `MOVNTI`/`vmovntps`.
3. On targets where non-temporal stores have weaker ordering, the resulting
   store may have different observability under MFENCE/SFENCE — though for
   x86 SSE2 NT stores this is more performance than correctness, for some
   downstream consumers and target lowering pipelines that translate
   `!nontemporal` into ordering-relevant primitives, this could affect
   correctness as well.

## Fix sketch

Either
* Make `StoreExpression::equals` also compare `hasMetadata(MD_nontemporal)`
  (and ideally any other store-flavor metadata that affects codegen), so the
  two stores end up in different congruence classes; or
* In `performSymbolicStoreEvaluation`, when the matching `LastCC` survives,
  promote the kept store's `!nontemporal` to the union of the two (i.e.,
  preserve `!nontemporal` if EITHER store has it), instead of relying on
  `combineMetadataForCSE`'s intersect-style merge.
