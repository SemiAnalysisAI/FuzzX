# w78 -- SROA vector-promotion `isVectorPromotionViable` ignores `isAtomic()`

## Component

`llvm/lib/Transforms/Scalar/SROA.cpp`, the vector-promotion eligibility
predicate at lines 2040-2080 (and the symmetric integer-widening predicate
at lines 2348-2398).

The relevant check for loads (line 2053) and stores (line 2066):

```cpp
} else if (LoadInst *LI = dyn_cast<LoadInst>(U->getUser())) {
  if (LI->isVolatile())
    return false;
  ...
} else if (StoreInst *SI = dyn_cast<StoreInst>(U->getUser())) {
  if (SI->isVolatile())
    return false;
  ...
```

There is **no** `isAtomic()` filter. An atomic-but-not-volatile load or
store is therefore accepted into the vector-promotion path. When SROA
later substitutes vector extract/insert/cast IR for the access, the
atomic ordering is dropped.

Same structural defect as w61 / w78 tree-merge but on a third code path.

## Reproducer

`/tmp/w78/sroa_intwide_atomic.ll`:

```ll
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

%S = type { i32, i32 }

define i32 @atomic_int_widen(i32 %v0, i32 %v1) {
  %a = alloca %S, align 8
  %p0 = getelementptr %S, ptr %a, i64 0, i32 0
  %p1 = getelementptr %S, ptr %a, i64 0, i32 1
  store i32 %v0, ptr %p0, align 4
  store atomic i32 %v1, ptr %p1 unordered, align 4
  %ld = load i64, ptr %a, align 8
  %lo = trunc i64 %ld to i32
  %hi = lshr i64 %ld, 32
  %hi32 = trunc i64 %hi to i32
  %s = add i32 %lo, %hi32
  ret i32 %s
}
```

## opt diff

`opt -passes=sroa` lowers the entire alloca to scalars; the `store
atomic ... unordered` is replaced by plain integer `zext`/`shl`/`or`,
losing the atomic ordering entirely:

```ll
define i32 @atomic_int_widen(i32 %v0, i32 %v1) {
  %a.sroa.2.0.insert.ext = zext i32 %v1 to i64
  %a.sroa.2.0.insert.shift = shl i64 %a.sroa.2.0.insert.ext, 32
  %a.sroa.2.0.insert.mask = and i64 undef, 4294967295
  %a.sroa.2.0.insert.insert = or i64 %a.sroa.2.0.insert.mask, %a.sroa.2.0.insert.shift
  %a.sroa.0.0.insert.ext = zext i32 %v0 to i64
  %a.sroa.0.0.insert.mask = and i64 %a.sroa.2.0.insert.insert, -4294967296
  %a.sroa.0.0.insert.insert = or i64 %a.sroa.0.0.insert.mask, %a.sroa.0.0.insert.ext
  ...
}
```

No more atomic op anywhere.

## llc diff

Pre-SROA llc emits a plain mov for the non-atomic store, but the atomic
store goes through atomic codegen scheduling. Post-SROA llc generates a
single non-atomic packed integer assembly with no MFENCE / `xchg` / atomic
sequencing.

## Caveat

Same as w78 tree-merge: the alloca is local-scoped, so atomic-on-alloca is
in-principle foldable. The risk surfaces the moment the alloca address
escapes via capture/launder/etc., because then the dropped ordering is
visible to other threads. The bug is a missing `isAtomic()` guard in the
viability predicate -- the same family that fired in w61 (rewriter
proper) and the new w78-tree-merge candidate.

## Fix sketch

In both `isVectorPromotionViable` (around line 2054/2067) and
`isIntegerWideningViableForSlice` (around line 2349/2375), strengthen the
predicate to:

```cpp
if (LI->isVolatile() || LI->isAtomic())
  return false;
```

so any atomic access disqualifies the alloca from these merging paths and
the more conservative slice rewriter handles it (where w61 also needs its
own fix).
