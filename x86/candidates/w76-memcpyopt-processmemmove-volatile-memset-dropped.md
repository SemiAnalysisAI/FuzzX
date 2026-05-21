# MemCpyOpt processMemMove drops volatile flag of preceding memset

**Component:** llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp

**Function:** `MemCpyOptPass::processMemMove` (line 1975) + `processMemSetMemCpyDependence` (line 1288)

**Pattern:** A volatile memset followed by a non-overlapping memmove
gets transformed such that the volatile flag is silently dropped.

## Bug

`processMemMove` checks whether the memmove's source overlaps its destination
via `isModSet(AA->getModRefInfo(M, MemoryLocation::getForSource(M)))`.  When
the source and destination are non-overlapping (e.g. memmove(p, p+16, 16)),
the memmove is converted to memcpy by `setCalledFunction`.

On the next iteration `processMemCpy` runs on the new memcpy. It finds a
preceding memset as the source clobber and calls
`processMemSetMemCpyDependence`, which never checks `MemSet->isVolatile()`
(this is the existing w53 bug).  The original VOLATILE memset is erased and
replaced with a non-volatile memset emitted by `Builder.CreateMemSet(...)`
which defaults `isVolatile=false`.

The novelty here vs w53: the entry point is a **memmove**, not a memcpy.
A user that uses memmove instead of memcpy (because they expect overlap
support) is not protected — the memmove-to-memcpy conversion makes the
volatile loss reachable from a different starting IR.

## Confirmed via opt

```ll
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg)
declare void @llvm.memmove.p0.p0.i64(ptr nocapture writeonly, ptr nocapture readonly, i64, i1 immarg)

define void @test_v(ptr %p) {
  call void @llvm.memset.p0.i64(ptr align 8 %p, i8 0, i64 64, i1 true)
  ret void
}

define void @test_mm(ptr %p) {
  call void @llvm.memset.p0.i64(ptr align 8 %p, i8 0, i64 64, i1 true)
  %src = getelementptr i8, ptr %p, i64 16
  call void @llvm.memmove.p0.p0.i64(ptr align 8 %p, ptr align 8 %src, i64 16, i1 false)
  ret void
}
```

After `opt -passes=memcpyopt -S`:

```ll
define void @test_v(ptr %p) {
  call void @llvm.memset.p0.i64(ptr align 8 %p, i8 0, i64 64, i1 true)   ; preserved
  ret void
}

define void @test_mm(ptr %p) {
  %src = getelementptr i8, ptr %p, i64 16
  %1 = getelementptr i8, ptr %p, i64 16
  call void @llvm.memset.p0.i64(ptr align 8 %p, i8 0, i64 64, i1 false)  ; volatile DROPPED
  ret void
}
```

The `test_v` control shows a standalone volatile memset is preserved; in
`test_mm` the **same** memset, now adjacent to a memmove that gets
converted-then-folded, loses its volatile flag.  The memcpy is also dropped
(redundant), so the volatile observability of writes [0,64) is gone.

## Fix sketch

Either:
1. Make `processMemSetMemCpyDependence` reject volatile/atomic memsets (the
   missing guard from w53), or
2. Make `processMemMove`'s memmove→memcpy conversion bail when a clobbering
   memset is volatile (so the chain is not entered at all).

A minimal fix at the top of `processMemSetMemCpyDependence`:
```cpp
if (MemSet->isVolatile() || MemSet->isAtomic())
  return false;
```
matches the volatile guard already used in `processMemCpyMemCpyDependence`
(line 1106).
