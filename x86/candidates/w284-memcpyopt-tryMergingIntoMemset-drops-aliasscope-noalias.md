# MemCpyOpt tryMergingIntoMemset drops !alias.scope and !noalias from merged stores

**Component:** llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp

**Function:** `MemCpyOptPass::tryMergingIntoMemset`

**Lines:** 352-501. Memset emission at lines 475-477.

## Bug

When a run of byte-equivalent stores is merged into a single memset, no AA
metadata is propagated to the new memset:

```cpp
// line 475-477
AMemSet = Builder.CreateMemSet(StartPtr, ByteVal, Range.End - Range.Start,
                               Range.Alignment);
AMemSet->mergeDIAssignID(Range.TheStores);
```

Only `MD_DIAssignID` is merged. Per LangRef, the right approach for AAMD
when several accesses are merged is to *intersect* `!alias.scope` and
*union* `!noalias` across the merged set — the `combineAAMetadata` /
`Instruction::mergeMetadata` machinery exists for exactly this. Today,
`tryMergingIntoMemset` just discards both.

The same issue applies to `!tbaa`/`!tbaa.struct` if all merged stores
agree, and to `!annotation`. Even if every store in the run carries
*identical* `!alias.scope !1`, the merged memset is emitted with no
scope, widening the AA set.

This is orthogonal to w76 (which is about losing `!nontemporal` on the
merged stores), and not covered by it. Even after applying the w76 fix
(reject `!nontemporal` stores in the merge loop), the merge is allowed to
proceed for stores that carry `!alias.scope`/`!noalias`/`!tbaa`, and those
will still be discarded.

## Confirmed via opt

```ll
target triple = "x86_64-unknown-linux-gnu"

declare void @use(ptr)

define void @test(ptr noalias %dst) {
  %p0 = getelementptr i8, ptr %dst, i64 0
  %p1 = getelementptr i8, ptr %dst, i64 8
  %p2 = getelementptr i8, ptr %dst, i64 16
  store i64 0, ptr %p0, align 8, !alias.scope !0
  store i64 0, ptr %p1, align 8, !alias.scope !0
  store i64 0, ptr %p2, align 8, !alias.scope !0
  call void @use(ptr %dst)
  ret void
}

!0 = !{!1}
!1 = distinct !{!1, !2, !"scope"}
!2 = distinct !{!2, !"domain"}
```

After `opt -passes=memcpyopt -S`:

```ll
define void @test(ptr noalias %dst) {
  %p0 = getelementptr i8, ptr %dst, i64 0
  %p1 = getelementptr i8, ptr %dst, i64 8
  %p2 = getelementptr i8, ptr %dst, i64 16
  call void @llvm.memset.p0.i64(ptr align 8 %p0, i8 0, i64 24, i1 false)
                                                ; no !alias.scope
  call void @use(ptr %dst)
  ret void
}
```

All three stores agreed on `!alias.scope !0`; the merged memset has no
scope, so downstream AA pessimistically assumes it can alias any noalias
region in the function.

## Fix sketch

After the `mergeDIAssignID` call, do something like:

```cpp
SmallVector<Instruction *> SrcInsts;
for (Instruction *SI : Range.TheStores)
  SrcInsts.push_back(SI);
AMemSet->mergeDIAssignID(Range.TheStores);
combineAAMetadata(AMemSet, SrcInsts);  // intersect/union AAMD
```

(`combineAAMetadata` overload that takes a range, or do a fold of
`combineAAMetadata(AMemSet, SrcInst)` over the range.) Also propagate
`!tbaa`/`!tbaa.struct` and `!annotation` consistently with other
merge sites in MemCpyOpt.
