# MemCpyOpt processMemSetMemCpyDependence drops AAMD from the replacement memset

**Component:** llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp

**Function:** `MemCpyOptPass::processMemSetMemCpyDependence`

**Lines:** 1288-1384. New memset built at lines 1369-1371.

## Bug

The transform shortens a redundant `memset(dst, c, dst_size)` that is
followed by `memcpy(dst, src, src_size)` (with `src_size <= dst_size`) by
moving the memset to `dst + src_size` for the remaining `dst_size -
src_size` bytes:

```cpp
// line 1369-1371
Instruction *NewMemSet =
    Builder.CreateMemSet(Builder.CreatePtrAdd(Dest, SrcSize),
                         MemSet->getOperand(1), MemsetLen, Alignment);
```

No metadata is propagated from `MemSet` to `NewMemSet`. The original memset
is then erased at line 1382 with no `combineAAMetadata` call. Therefore the
following are silently dropped from the new memset:

- `!alias.scope`, `!noalias` (AA-relevant correctness regression)
- `!tbaa`, `!tbaa.struct`
- `MD_DIAssignID` and `!annotation`

Sibling transforms in the same file all do at least the DIAssignID copy
(line 685, 809, 1256). This one does nothing.

This is orthogonal to w53 (which is about losing `isVolatile`/atomicity on
the memset). Even if the volatility fix lands, this AAMD-loss is
independent and still broken.

## Confirmed via opt

```ll
target triple = "x86_64-unknown-linux-gnu"

declare void @use(ptr)
declare void @llvm.memcpy.p0.p0.i64(ptr captures(none), ptr captures(none),
                                    i64, i1)
declare void @llvm.memset.p0.i64(ptr captures(none), i8, i64, i1)

define void @test(ptr %src) {
  %dst = alloca [32 x i8], align 8
  call void @llvm.memset.p0.i64(ptr align 8 %dst, i8 0, i64 32, i1 false),
       !alias.scope !0, !noalias !4
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src,
                                   i64 16, i1 false)
  call void @use(ptr %dst)
  ret void
}

!0 = !{!1}
!1 = distinct !{!1, !2, !"scope"}
!2 = distinct !{!2, !"domain"}
!4 = !{!5}
!5 = distinct !{!5, !2, !"scope2"}
```

After `opt -passes=memcpyopt -S`:

```ll
define void @test(ptr %src) {
  %dst = alloca [32 x i8], align 8
  %0 = getelementptr i8, ptr %dst, i64 16
  call void @llvm.memset.p0.i64(ptr align 8 %0, i8 0, i64 16, i1 false)
                                                ; no !alias.scope, no !noalias
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src,
                                   i64 16, i1 false)
  ...
}
```

## Fix sketch

```cpp
NewMemSet->copyMetadata(*MemSet, LLVMContext::MD_DIAssignID);
combineAAMetadata(NewMemSet, MemSet);
```

This is the bare minimum to match sibling transforms; the volatility/atomic
side (w53) needs its own guards.
