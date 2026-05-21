# w363: ScalarizeMaskedMemIntrin scalarizeMaskedCompressStore drops ALL metadata on every path

## Status: confirmed (reproducer; both constant- and dynamic-mask paths drop)

## Where (source:lines)

`llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp`:

`scalarizeMaskedCompressStore`:
- **Constant-mask** path, line **877**:
  ```cpp
  Builder.CreateAlignedStore(OneElt, NewPtr, AdjustedAlignment);
  ```
  No `StoreInst*` capture, no metadata copy.
- **Dynamic-mask** path, line **927**:
  ```cpp
  Builder.CreateAlignedStore(OneElt, Ptr, AdjustedAlignment);
  ```
  No metadata copy.

Like `scalarizeMaskedExpandLoad` (w362), there is no all-true / splat
short-cut in compressstore at all. Every compressstore lowered through this
pass loses all metadata, regardless of mask form.

## Reproducer (const mask)

`/tmp/w360/compressstore-const.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
define void @compressstore_const(<4 x i32> %v, ptr %p) {
  call void @llvm.masked.compressstore.v4i32(
        <4 x i32> %v, ptr %p, <4 x i1> <i1 true, i1 true, i1 false, i1 true>),
        !nontemporal !0, !noalias !1
  ret void
}
declare void @llvm.masked.compressstore.v4i32(<4 x i32>, ptr, <4 x i1>)
!0 = !{i32 1}
!1 = !{!2}
!2 = distinct !{!2, !3, !"scope"}
!3 = distinct !{!3, !"domain"}
```

After `opt -passes=scalarize-masked-mem-intrin -mtriple=x86_64--`:

```llvm
store i32 %Elt0, ptr %1, align 1     ; no !nontemporal, no !noalias
store i32 %Elt1, ptr %2, align 1     ; no !nontemporal, no !noalias
store i32 %Elt3, ptr %3, align 1     ; no !nontemporal, no !noalias
```

## Reproducer (dynamic mask)

`/tmp/w360/compressstore-dyn.ll`:

```llvm
define void @compressstore_dyn(<4 x i32> %v, ptr %p, <4 x i1> %m) {
  call void @llvm.masked.compressstore.v4i32(
        <4 x i32> %v, ptr %p, <4 x i1> %m), !nontemporal !0, !noalias !1
  ret void
}
```

Output emits four conditional `store i32 ..., ptr %..., align 1` per-lane
stores, all bare.

## Observable consequences

- `!nontemporal`: streaming stores become cached stores (cache pollution +
  different memory ordering visibility properties).
- `!noalias` / `!alias.scope`: later AA queries cannot disambiguate the
  per-lane stores from adjacent loads. This blocks LICM hoisting / SLP
  rebuild / store-to-load forwarding optimizations that the original
  compressstore intrinsic was annotated to enable.
- TBAA: same.
- `!annotation` / `!mmra`: lost entirely.

## Where to fix

- Line 877 (constant mask):
  ```cpp
  StoreInst *S = Builder.CreateAlignedStore(OneElt, NewPtr, AdjustedAlignment);
  S->copyMetadata(*CI);
  ```
- Line 927 (dynamic mask): same shape.

## Triage notes

Sibling to w362 (expandload ↔ compressstore, load ↔ store). Same shape, same
fix template. Natural to bundle both into one PR alongside w360/w361.
