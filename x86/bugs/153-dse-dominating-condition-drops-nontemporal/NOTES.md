# DSE `eliminateRedundantStoresViaDominatingConditions` silently drops `!nontemporal` store

**Component:** llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp

**Function:** `DSEState::eliminateRedundantStoresViaDominatingConditions()`

**Lines:** 2143-2249 (function body); deletion at 2216
(`deleteDeadInstruction(SI)`); guard at 2199 (`SI->isUnordered()`).

**Pattern:** A `store` instruction is guarded by a dominating equality comparison
between a loaded value and the value-to-be-stored. DSE treats such a store as
a no-op when the value is implied by the dominating condition and no
clobbering write happens between the load and the store. The deletion path
checks only `SI->isUnordered()` - it does **not** check `SI->hasMetadata(MD_nontemporal)`,
so an x86 cache-bypassing store (`movntiq`) is silently dropped.

## Bug

Lines 2198-2199:
```
auto *SI = dyn_cast<StoreInst>(Def->getMemoryInst());
if (!SI || !SI->isUnordered())
  continue;
```
`isUnordered()` is true for plain stores and also for stores carrying
`!nontemporal`. The pass concludes the store is a no-op and at line 2216
calls `deleteDeadInstruction(SI)`. The `!nontemporal` hint - and the
cache-bypassing semantics it carries on x86 - is lost together with the store.

## Confirmed via opt + llc (x86_64)

### Input IR
```ll
target triple = "x86_64-unknown-linux-gnu"
define void @t(ptr %p) {
  %v = load i64, ptr %p, align 8
  %c = icmp eq i64 %v, 42
  br i1 %c, label %t, label %f
t:
  store i64 42, ptr %p, align 8, !nontemporal !0
  ret void
f:
  ret void
}
!0 = !{i32 1}
```

After `opt -passes=dse`:
```ll
define void @t(ptr %p) {
  %v = load i64, ptr %p, align 8
  %c = icmp eq i64 %v, 42
  br i1 %c, label %t, label %f
t:
  ret void
f:
  ret void
}
```
The NT store is gone.

### llc output (x86_64, +sse2)

**Without DSE (original IR):**
```
cmpq    $42, (%rdi)
jne     .LBB0_2
# %bb.1:
movl    $42, %eax
movntiq %rax, (%rdi)     ; <-- non-temporal store (cache-bypassing)
.LBB0_2:
retq
```

**After DSE:**
```
cmpq    $42, (%rdi)
retq                       ; <-- NO STORE AT ALL
```

The `movntiq` cache-bypassing store is silently dropped. NT stores:
* bypass the cache (different cache-state effect)
* are weakly ordered w.r.t. older stores and require `sfence` to be observed by
  other CPUs in the architectural memory model

For driver / MMIO / write-combining-memory code that relies on these semantics
(e.g., flushing a cache line, writing to WC framebuffers, MMIO PCIe BARs), this
is an observable miscompile.

## Fix sketch

Add a metadata-presence check in
`eliminateRedundantStoresViaDominatingConditions` before deleting:
```cpp
if (SI->hasMetadata(LLVMContext::MD_nontemporal))
  continue;
```
Same defensive check should be added in any DSE path that deletes a store
whose value is "already in memory" (e.g., `storeIsNoop`,
`eliminateRedundantStoresOfExistingValues`, and the partial-merge branch).
`!nontemporal` is not a pure hint - it changes cache and memory-ordering
behavior on x86, so it should be treated like volatile/atomic for removability
purposes.
