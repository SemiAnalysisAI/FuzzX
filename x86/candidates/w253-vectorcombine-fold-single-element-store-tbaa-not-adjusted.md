# w253 — VectorCombine `foldSingleElementStore` copies vector-store TBAA to scalar element store without `adjustForAccess`

## Files / locations

- `llvm/lib/Transforms/Vectorize/VectorCombine.cpp:1989-1998`
  Function: `VectorCombine::foldSingleElementStore(Instruction &I)`

## Bug

`foldSingleElementStore` rewrites
```
%v   = load <4 x i32>, ptr %p, !tbaa !V       ; tag describes a <4 x i32> access
%ins = insertelement <4 x i32> %v, i32 %x, i32 1
       store <4 x i32> %ins, ptr %p, !tbaa !V ; tag describes a <4 x i32> access
```
into
```
%gep = getelementptr inbounds <4 x i32>, ptr %p, i32 0, i32 1
       store i32 %x, ptr %gep, ...           ; scalar i32 access, offset 4
```

The relevant code:
```cpp
StoreInst *NSI = Builder.CreateStore(NewElement, GEP);
NSI->copyMetadata(*SI);              // <-- copies vector-typed !tbaa verbatim
Align ScalarOpAlignment = computeAlignmentAfterScalarization(...);
NSI->setAlignment(ScalarOpAlignment);
```

The companion routine `scalarizeLoadExtract` at line 2138-2139 correctly
does
```cpp
AAMDNodes OldAAMD = LI->getAAMetadata();
NewLoad->setAAMetadata(OldAAMD.adjustForAccess(Offset, ElemType, *DL));
```
but `foldSingleElementStore` skips that adjustment. The resulting scalar
store inherits a `!tbaa` tag whose access-type and offset describe a
vector access starting at byte 0, not a scalar access starting at byte 4.

## Reproducer

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @f(ptr %p, i32 %x) {
  %v = load <4 x i32>, ptr %p, !tbaa !0
  %ins = insertelement <4 x i32> %v, i32 %x, i32 1
  store <4 x i32> %ins, ptr %p, !tbaa !0
  ret void
}

!0 = !{!1, !1, i64 0}              ; access-type "int[]", offset 0
!1 = !{!"int[]", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C/C++ TBAA"}
```

`opt -passes='vector-combine' -S` produces:

```llvm
define void @f(ptr %p, i32 %x) {
  %1 = getelementptr inbounds <4 x i32>, ptr %p, i32 0, i32 1
  store i32 %x, ptr %1, align 4, !tbaa !0
  ret void
}

!0 = !{!1, !1, i64 0}              ; still tag for vector at offset 0
!1 = !{!"int[]", !2, i64 0}
```

The scalar `store i32 %x, ptr (p+4)` is tagged with a TBAA path whose base
type is `int[]` and access-offset is `0`. Downstream TBAA queries that
intersect this tag against a struct-field access for the *non*-offset-4
element of the same int[] may incorrectly conclude no-alias, because the
recorded offset does not match the actual byte offset of the store.

## Why this is wrong

- The transform changes the access type and offset (vector tag at offset 0
  → scalar `int` tag at offset 4), so the TBAA tag must be recomputed with
  `AAMDNodes::adjustForAccess(Offset, ElemType, *DL)`, exactly as the
  sister routine `scalarizeLoadExtract` does. Skipping it can mis-alias.
- The same problem applies to TBAA-struct metadata; both are part of
  `AAMDNodes`. `adjustForAccess` exists precisely so a scalarizing
  transform can fix the access tag.

## Fix sketch

Replace
```cpp
NSI->copyMetadata(*SI);
```
with the offset-aware pattern:
```cpp
NSI->copyMetadata(*SI);                                    // keep scope/noalias
AAMDNodes OldAAMD = SI->getAAMetadata();
if (auto *CI = dyn_cast<ConstantInt>(Idx)) {
  size_t Off = CI->getZExtValue() * DL->getTypeStoreSize(NewElement->getType());
  NSI->setAAMetadata(OldAAMD.adjustForAccess(Off, NewElement->getType(), *DL));
} else {
  NSI->setAAMetadata(OldAAMD);  // can't recompute offset; keep noalias only
}
```
