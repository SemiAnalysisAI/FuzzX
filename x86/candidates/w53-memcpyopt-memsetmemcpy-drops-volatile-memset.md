# MemCpyOpt processMemSetMemCpyDependence eliminates volatile/atomic MemSet

**Component:** llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp

**Function:** `MemCpyOptPass::processMemSetMemCpyDependence`

**Lines:** 1288-1384

**Pattern:** MemCpyOpt merges into a memcpy and drops volatile

## Bug

`processMemSetMemCpyDependence` is called from `processMemCpy` (line 1850)
after verifying the **MemCpy** is non-volatile (line 1796 `M->isVolatile()`).
But the function never checks `MemSet->isVolatile()` or whether MemSet is an
element-atomic mem intrinsic.

The transform replaces:
```
memset(dst, c, dst_size)   ; possibly VOLATILE
memcpy(dst, src, src_size) ; src_size <= dst_size, non-volatile
```
with
```
memset(dst + src_size, c, dst_size - src_size)  ; non-volatile
memcpy(...)
```
The original memset is **erased** (line 1382), and the new memset is created
via `Builder.CreateMemSet(...)` which defaults to `isVolatile=false` (line
1369-1371). The first `src_size` bytes that the volatile memset wrote are
silently dropped, **and** the remaining bytes lose their volatile flag.

The `DestSize == SrcSize` fast path at line 1326-1329 drops the volatile
memset entirely with no replacement.

There is no `isAtomic()` check either, so an atomic
`memset.element.unordered.atomic` could be replaced with a non-atomic memset,
violating atomicity guarantees.

## Confirmed via opt

```ll
define void @test1(ptr %src) {
  %dst = alloca [32 x i8], align 8
  call void @llvm.memset.p0.i64(ptr align 8 %dst, i8 0, i64 32, i1 true)   ; VOLATILE
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src, i64 16, i1 false)
  call void @use(ptr %dst)
  ret void
}
```

After `opt -passes=memcpyopt`:
```ll
define void @test1(ptr %src) {
  %dst = alloca [32 x i8], align 8
  %1 = getelementptr i8, ptr %dst, i64 16
  call void @llvm.memset.p0.i64(ptr align 8 %1, i8 0, i64 16, i1 false)    ; NON-VOLATILE
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src, i64 16, i1 false)
  call void @use(ptr %dst)
  ret void
}
```

Original wrote 32 volatile bytes; now writes 16 non-volatile bytes. The
volatile observability of bytes [0, 16) is gone, and bytes [16, 32) lost
their volatile flag.

With equal sizes (32-byte memset, 32-byte memcpy), the volatile memset is
deleted outright via the `DestSize == SrcSize` branch.

## Fix sketch

Add at the top of `processMemSetMemCpyDependence`:
```cpp
if (MemSet->isVolatile() || MemSet->isAtomic())
  return false;
```
Same pattern is used in sibling functions, e.g. line 1106
(`MDep->isVolatile()`).
