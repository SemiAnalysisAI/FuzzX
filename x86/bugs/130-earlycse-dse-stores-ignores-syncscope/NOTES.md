# EarlyCSE DSE drops earlier atomic store with different syncscope

## Location
`llvm/lib/Transforms/Scalar/EarlyCSE.cpp` — `EarlyCSE::overridingStores` (lines 1335-1367) called from the DSE block at lines 1755-1773.

## Root cause
`overridingStores` decides whether `Later` can subsume `Earlier`. It checks pointer, value-type, matching-id, and that both are unordered/non-volatile:

```cpp
if (!Earlier.isUnordered() || !Later.isUnordered())
    return false;
```

It never compares `SyncScope::ID`. So two `store atomic unordered` instructions on the same pointer with different syncscopes are treated as overridable, and the earlier one is deleted by the DSE block in `processNode`.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"
define void @f(ptr %p, i32 %v1, i32 %v2) {
  store atomic i32 %v1, ptr %p syncscope("singlethread") unordered, align 4
  store atomic i32 %v2, ptr %p syncscope("system") unordered, align 4
  ret void
}
```

## opt diff
Before:
```
store atomic i32 %v1, ptr %p syncscope("singlethread") unordered, align 4
store atomic i32 %v2, ptr %p syncscope("system") unordered, align 4
```

After `opt -passes=early-cse`:
```
store atomic i32 %v2, ptr %p syncscope("system") unordered, align 4
```

The `singlethread` store was silently deleted.

## Why it's wrong
Even with two unordered stores, deleting the first changes the set of observable atomic events at that location. Per LangRef, syncscope identifies the synchronization set that the operation participates in; the two stores are semantically distinct events, and the comment in `overridingStores` ("we were going to execute the non-atomic one anyway") implicitly assumes both stores reach the same observer set. With differing syncscopes the assumption breaks: a thread inside the `singlethread` scope (e.g. the issuing thread alone for non-x86 backends, or interrupt-handler-style models) may legitimately observe %v1 separately from a `system`-scoped observer that only sees %v2.

This is the store-side analog of the load-side issue (`w87-earlycse-load-cse-ignores-syncscope.md`); both stem from EarlyCSE's `LoadValue`/`ParseMemoryInst` cache not tracking syncscope.

## Suggested fix
Add a syncscope-equality check to `overridingStores`, e.g.:
```cpp
if (Earlier.get()->isAtomic() && Later.get()->isAtomic()) {
  auto getSync = [](const Instruction *I) {
    if (auto *SI = dyn_cast<StoreInst>(I)) return SI->getSyncScopeID();
    if (auto *LI = dyn_cast<LoadInst>(I)) return LI->getSyncScopeID();
    return SyncScope::System;
  };
  if (getSync(Earlier.get()) != getSync(Later.get()))
    return false;
}
```
(plus the analogous tracking in `LoadValue` for the load-CSE side).

## Status: REPRODUCIBLE (IR-level miscompile, opt-only)
