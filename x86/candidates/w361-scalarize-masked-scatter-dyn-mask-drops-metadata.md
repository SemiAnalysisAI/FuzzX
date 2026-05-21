# w361: ScalarizeMaskedMemIntrin scalarizeMaskedScatter (dynamic-mask) drops ALL metadata on per-lane stores

## Status: confirmed (reproducer; observable codegen regression for `!nontemporal`)

## Where (source:lines)

`llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp`:
- `scalarizeMaskedScatter`, dynamic-mask loop, line **690-692**:
  ```cpp
  Value *OneElt = Builder.CreateExtractElement(Src, Idx, "Elt" + Twine(Idx));
  Value *Ptr = Builder.CreateExtractElement(Ptrs, Idx, "Ptr" + Twine(Idx));
  Builder.CreateAlignedStore(OneElt, Ptr, AlignVal);
  ```
  No metadata copy.

Compare with `scalarizeMaskedStore` splat-mask path at line 376 (`Store->copyMetadata(*CI)`) — that path *does* copy. The scatter equivalents never do.

## Reproducer

`/tmp/w360/scatter-dyn-nontemporal.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
define void @scatter_dyn_nontemporal(<4 x i32> %v, <4 x ptr> %ptrs, <4 x i1> %m) {
  call void @llvm.masked.scatter.v4i32.v4p0(
        <4 x i32> %v, <4 x ptr> %ptrs, i32 4, <4 x i1> %m), !nontemporal !0
  ret void
}
declare void @llvm.masked.scatter.v4i32.v4p0(<4 x i32>, <4 x ptr>, i32, <4 x i1>)
!0 = !{i32 1}
```

### After `opt -passes=scalarize-masked-mem-intrin -mtriple=x86_64--`

All four per-lane stores look like `store i32 %EltN, ptr %PtrN, align 4` — `!nontemporal` is gone.

### Codegen confirms (opt -> llc, -mattr=+avx2):

```
vmovss  %xmm0, (%rcx)              ; lane 0 store (no streaming)
vextractps $1, %xmm0, (%rcx)       ; lane 1 store
vmovq   %xmm1, %rcx
vextractps $2, %xmm0, (%rcx)       ; lane 2 store
vextractps $3, %xmm0, (%rax)       ; lane 3 store
```

These should be non-temporal stores (`movnti`/`movntdq`) because the user asked
for streaming via `!nontemporal !0`. The scatter intrinsic carried it; the
scalarization threw it away; codegen has nothing to lower.

This is an observable runtime regression — non-temporal stores bypass the
caches, and emitting cached stores instead pollutes L1/L2 and behaves
qualitatively differently (visibility, write-combining), exactly the property
the user opted into.

## Other metadata silently dropped on the per-lane stores

`!nontemporal`, `!noalias`, `!alias.scope`, `!annotation`, `!mmra`, TBAA,
`!invariant.group` (relevant for derived stores). For aliasing metadata in
particular, dropping `!noalias` on a per-lane store can let a later AA query
*pessimistically* couple it with an unrelated load — observable through MIR
scheduling / DAG combine.

## Where to fix

After line 692 add:
```cpp
StoreInst *Store = Builder.CreateAlignedStore(OneElt, Ptr, AlignVal);
Store->copyMetadata(*CI);
```

(Change the unused-result expression to a named `StoreInst*` first.)

## Triage notes

Sibling to w360 (scatter ↔ gather, store ↔ load). Bundling these into one PR
with the gather fix is the natural shape.
