# w61 -- SimplifyCFG `mergeConditionalStores` drops `atomic` from Unordered atomic stores

## Component

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` `mergeConditionalStoresEdge`
(line 4408) inside `mergeConditionalStores` (line 4434).

The upstream filter at line 4275 is:

```cpp
if (!QStore->isUnordered() || !PStore->isUnordered() || ...)
  return false;
```

`Store::isUnordered()` returns true for both **NotAtomic** and **Unordered**
ordering (and excludes volatile). So `Unordered` atomic stores pass the
filter, but at line 4408 the merged store is created with a plain
`CreateStore`:

```cpp
StoreInst *SI = cast<StoreInst>(QB.CreateStore(QPHI, Address));
```

No `setAtomic()` call ever follows. The merged store is therefore a
non-atomic store, silently dropping the `unordered` ordering from the
original stores.

## Repro

`/tmp/w61/simplifycfg_atomic.ll`:

```ll
target triple = "x86_64-unknown-linux-gnu"

define void @merge_cond_stores_atomic(i1 %c1, i1 %c2, ptr %p) {
entry:
  br i1 %c1, label %if.then, label %if.else
if.then:
  store atomic i32 1, ptr %p unordered, align 4
  br label %merge
if.else:
  br label %merge
merge:
  br i1 %c2, label %if.then2, label %if.end
if.then2:
  store atomic i32 2, ptr %p unordered, align 4
  br label %if.end
if.end:
  ret void
}
```

## Invocation

```
opt -passes='simplifycfg<>' -S simplifycfg_atomic.ll
```

## Output

```
define void @merge_cond_stores_atomic(i1 %c1, i1 %c2, ptr %p) {
entry:
  %spec.select = select i1 %c2, i32 2, i32 1
  %0 = or i1 %c1, %c2
  br i1 %0, label %1, label %2

1:                                                ; preds = %entry
  store i32 %spec.select, ptr %p, align 4    ; <-- atomic dropped!
  br label %2
...
}
```

The two original `store atomic i32 ... unordered` were merged into a
single plain `store i32`, dropping the `atomic` qualifier. While the
unordered ordering itself does not enforce inter-thread ordering, the
`atomic` qualifier *does* affect race semantics: data races on
atomic Unordered memory are well-defined in LLVM IR, while races on
plain memory cause undefined behavior. The transform thus introduces UB
that was not present in the source program.

This is the same family as #015 (X86 SFB), #017
(widenPartwordAtomicRMW), #108 (DSE tryToMergePartialOverlappingStores),
#012 (CGP splitMergedValStore) and the new #w61-sroa bug -- a transform
that creates a new store via IRBuilder without faithfully propagating
the `atomic` bit.
