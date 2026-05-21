# MemCpyOpt processMemCpy constant-GV-to-memset transform drops AAMD and !nontemporal

**Component:** llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp

**Function:** `MemCpyOptPass::processMemCpy` (the GV byte-splat -> memset transform)

**Lines:** 1818-1834 (transform). Memset built at line 1824-1825.

## Bug

When `processMemCpy` recognizes that the source of a memcpy is a
constant global whose initializer is a single repeated byte, it replaces
the memcpy with a memset:

```cpp
// line 1823-1825
IRBuilder<> Builder(M);
Instruction *NewM = Builder.CreateMemSet(
    M->getRawDest(), ByteVal, M->getLength(), M->getDestAlign(), false);
```

No metadata is propagated from the original memcpy to the new memset. The
original memcpy may carry:

- `!alias.scope`, `!noalias`, `!tbaa` (AA-relevant)
- `!nontemporal` (memcpy doesn't have a LangRef-level nontemporal form, but
  if the optimizer is going to emit memset, then dropping the user-supplied
  hint is silent)
- `MD_DIAssignID` (sibling transforms preserve this consistently)

The original memcpy is then erased at line 1831 with no metadata
preservation whatsoever. Compare with `processMemCpyMemCpyDependence`
(line 1256), `processStoreOfLoad` (line 685), and the store->memset
transform at line 809, which all copy at least `MD_DIAssignID`.

## Confirmed via opt

```ll
target triple = "x86_64-unknown-linux-gnu"

@C = private constant [16 x i8] zeroinitializer

declare void @use(ptr)
declare void @llvm.memcpy.p0.p0.i64(ptr captures(none), ptr captures(none),
                                    i64, i1)

define void @test(ptr noalias %dst) {
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 @C,
                                   i64 16, i1 false),
       !alias.scope !0, !noalias !4
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
define void @test(ptr noalias %dst) {
  call void @llvm.memset.p0.i64(ptr align 8 %dst, i8 0, i64 16, i1 false)
                                                ; no !alias.scope, no !noalias
  call void @use(ptr %dst)
  ret void
}
```

Both AA scopes are silently dropped.

## Fix sketch

Mirror `processMemCpyMemCpyDependence`:

```cpp
NewM->copyMetadata(*M, LLVMContext::MD_DIAssignID);
combineAAMetadata(NewM, M);
```

(The transform also writes `isVolatile=false` hard-coded. The outer
`processMemCpy` does bail on `M->isVolatile()` at line 1796, so volatile is
sound. Atomicity is also impossible here because `MemCpyInst` excludes the
element-atomic variant — only volatile/AAMD are at risk.)
