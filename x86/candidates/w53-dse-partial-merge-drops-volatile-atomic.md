# DSE partial-overlap store merging eliminates volatile/atomic killing store

**Component:** llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp

**Function:** `DSEState::eliminateDeadDefs(MemoryLocationWrapper&)` partial-merge
branch + `tryToMergePartialOverlappingStores`

**Lines:** 2683-2708 (call site), 770-814 (helper)

**Pattern:** DSE eliminates a store that's actually live (cross-thread
observable)

## Bug

When `OR == OW_PartialEarlierWithFullLater` (KillingSI is fully contained
inside DeadSI and `EnablePartialStoreMerging` is on, which is the default),
the code at 2683 calls `tryToMergePartialOverlappingStores(KillingSI, DeadSI,
...)`. On success:
* `DeadSI->setOperand(0, Merged)` — mutates the earlier store's value, and
* `deleteDeadInstruction(KillingSI, &Deleted)` — **erases the killing store**.

`tryToMergePartialOverlappingStores` (770-814) only requires:
* both operands are `ConstantInt`,
* `typeSizeEqualsStoreSize` on both,
* `memoryIsNotModifiedBetween`.

It does **not** check `isSimple()` on KillingSI. The outer caller
(`eliminateDeadDefs(KillingDefWrapper)`, line 2727) also does not check
`isRemovable(KillingI)` before invoking the per-location elimination.
Only DeadSI is guaranteed `isRemovable` (verified inside `getDomMemoryDef`
line 1733 when walking dead candidates).

Result: a volatile or atomic-monotonic store of a constant integer can be
silently dropped and merged into a preceding simple store.

## Confirmed via opt

### Volatile case
```ll
define void @test(ptr %p) {
  store i32 0, ptr %p, align 4
  %p2 = getelementptr i8, ptr %p, i64 2
  store volatile i16 -1, ptr %p2, align 2
  ret void
}
```
After `opt -passes=dse`:
```ll
define void @test(ptr %p) {
  store i32 -65536, ptr %p, align 4
  ret void
}
```
The `store volatile i16 -1` is gone; its value was merged into a
**non-volatile** i32 store. A volatile MMIO write was eliminated.

### Atomic monotonic case
```ll
define void @test(ptr %p) {
  store i32 0, ptr %p, align 4
  %p2 = getelementptr i8, ptr %p, i64 2
  store atomic i16 -1, ptr %p2 monotonic, align 2
  ret void
}
```
After `opt -passes=dse`:
```ll
define void @test(ptr %p) {
  store i32 -65536, ptr %p, align 4
  ret void
}
```
The atomic monotonic store is silently replaced by a non-atomic store; the
cross-thread atomicity guarantee is lost. Other threads may now observe an
intermediate state of `%p` that the IR contract forbade.

## Fix sketch

Either:
* Gate the partial-merge branch on `KillingSI && KillingSI->isSimple() && DeadSI && DeadSI->isSimple()`, or
* Add `if (!KillingI->isSimple()) return false;` inside
  `tryToMergePartialOverlappingStores` (already takes a `StoreInst *`).

The companion full-overwrite path at line 2710 (`OR == OW_Complete`) is safe
because it relies on `isRemovable(DeadI)` which excludes volatile/atomic
stores via `SI->isUnordered()` (line 1457).
