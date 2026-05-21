# w61 -- SROA drops `atomic` ordering when rewriting a non-volatile atomic load/store

## Component

`llvm/lib/Transforms/Scalar/SROA.cpp`, the AllocaSliceRewriter pipeline.

The pre-filter `AllocaSliceRewriter::visitLoadInst` / `visitStoreInst` and
the integer-widening helpers (lines 2046-2076, 2349-2400) check `isVolatile()`
but never `isAtomic()`. An atomic-but-not-volatile access therefore enters the
slice rewriter.

The slice rewriter then synthesizes the new load/store and *conditionally*
copies the atomic ordering only when the original is **volatile**:

```cpp
// SROA.cpp:3165-3168  (load rewrite)
LoadInst *NewLI = IRB.CreateAlignedLoad(
    NewAllocaTy, NewPtr, NewAI.getAlign(), LI.isVolatile(), LI.getName());
if (LI.isVolatile())                                 // <-- WRONG predicate
  NewLI->setAtomic(LI.getOrdering(), LI.getSyncScopeID());
```

```cpp
// SROA.cpp:3204-3206  (load rewrite -- ptr-adjusted path)
if (LI.isVolatile())
  NewLI->setAtomic(LI.getOrdering(), LI.getSyncScopeID());
```

```cpp
// SROA.cpp:3371-3372  (store rewrite)
if (SI.isVolatile())
  NewSI->setAtomic(SI.getOrdering(), SI.getSyncScopeID());
```

The predicate `if (LI.isVolatile())` is meant to be `if (LI.isAtomic())`. As a
result an atomic-only load/store has its ordering silently discarded; the
new memory access becomes a plain (non-atomic, non-volatile) access.

## Reproducer

`/tmp/w61/sroa_atomic2.ll`:

```ll
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }

define i32 @atomic_load_from_partial(ptr %src) {
  %a = alloca %S, align 8
  %ld = load i64, ptr %src, align 8
  store i64 %ld, ptr %a, align 8
  %p1 = getelementptr inbounds %S, ptr %a, i32 0, i32 1
  %r = load atomic i32, ptr %p1 seq_cst, align 4
  ret i32 %r
}

define void @atomic_store_to_partial(ptr %dst, i32 %x) {
  %a = alloca %S, align 8
  store atomic i32 %x, ptr %a seq_cst, align 4
  %p1 = getelementptr inbounds %S, ptr %a, i32 0, i32 1
  store i32 0, ptr %p1, align 4
  %v = load i64, ptr %a, align 8
  store i64 %v, ptr %dst, align 8
  ret void
}
```

## Invocation

```
opt -passes=sroa -S sroa_atomic2.ll
```

## Output

```
define i32 @atomic_load_from_partial(ptr %src) {
  %ld = load i64, ptr %src, align 8
  %a.sroa.0.0.extract.trunc = trunc i64 %ld to i32
  %a.sroa.1.0.extract.shift = lshr i64 %ld, 32
  %a.sroa.1.0.extract.trunc = trunc i64 %a.sroa.1.0.extract.shift to i32
  ret i32 %a.sroa.1.0.extract.trunc       ; <-- atomic gone, just plain trunc
}

define void @atomic_store_to_partial(ptr %dst, i32 %x) {
  ...
  store i64 %a.sroa.0.0.insert.insert, ptr %dst, align 8    ; <-- atomic gone
  ret void
}
```

The first function actually folds the atomic load into pure integer
arithmetic; the second collapses the atomic store into a non-atomic 64-bit
store. The synchronization ordering (`seq_cst`) has been dropped without any
fence emitted.

Although both reduced cases above are technically OK on this specific
alloca-only memory, the *general* bug is reachable whenever SROA is forced to
go through the slice-rewriter path with `atomic` loads/stores. The miscompile
also surfaces in more elaborate shapes (e.g. an atomic store to a slice that
ends up replaced by a non-atomic store into the resulting promoted alloca's
backing memory or when SROA decides not to fully promote).

This is the same family of bug as #015/#017/#108/#111/#012/#109 -- a transform
that creates a new load/store via IRBuilder without faithfully propagating
the `atomic` bit.
