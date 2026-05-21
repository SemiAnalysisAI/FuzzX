# w362: ScalarizeMaskedMemIntrin scalarizeMaskedExpandLoad drops ALL metadata on every path

## Status: confirmed (reproducer; observable; both constant- and dynamic-mask paths drop)

## Where (source:lines)

`llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp`:

`scalarizeMaskedExpandLoad`:
- **Constant-mask** path, line **748-749**:
  ```cpp
  InsertElt = Builder.CreateAlignedLoad(EltTy, NewPtr, AdjustedAlignment,
                                        "Load" + Twine(Idx));
  ```
  No metadata copy. Even worse: in the const-mask gather case (w104) the *vector* alternative still loses metadata, but here `expandload` has *no* "all-true" or "splat" short-cut at all, so even the easy case never propagates anything.
- **Dynamic-mask** path, line **806**:
  ```cpp
  LoadInst *Load = Builder.CreateAlignedLoad(EltTy, Ptr, AdjustedAlignment);
  ```
  No metadata copy.

There is **no** `copyMetadata` anywhere in the expandload function. Unlike
`scalarizeMaskedLoad` which has three code paths (all-true ✓, splat ✓,
per-lane chain ✓ on the head load only), expandload has zero.

## Reproducer (const mask)

`/tmp/w360/expandload-const.ll`:
```llvm
target triple = "x86_64-unknown-linux-gnu"
define <4 x i32> @expandload_const(ptr %p, <4 x i32> %pt) {
  %v = call <4 x i32> @llvm.masked.expandload.v4i32(
        ptr %p, <4 x i1> <i1 true, i1 false, i1 true, i1 false>, <4 x i32> %pt),
        !range !0, !nontemporal !1
  ret <4 x i32> %v
}
declare <4 x i32> @llvm.masked.expandload.v4i32(ptr, <4 x i1>, <4 x i32>)
!0 = !{i32 0, i32 10}
!1 = !{i32 1}
```

After `opt -passes=scalarize-masked-mem-intrin -mtriple=x86_64--`:
```llvm
%Load0 = load i32, ptr %1, align 1          ; no !range, no !nontemporal
%Load2 = load i32, ptr %2, align 1          ; no !range, no !nontemporal
```

## Reproducer (dynamic mask)

`/tmp/w360/expandload-dyn.ll`:
```llvm
define <4 x i32> @expandload_dyn(ptr %p, <4 x i1> %m, <4 x i32> %pt) {
  %v = call <4 x i32> @llvm.masked.expandload.v4i32(
        ptr %p, <4 x i1> %m, <4 x i32> %pt),
        !range !0, !nontemporal !1, !noalias !2
  ret <4 x i32> %v
}
```

Output has four `load i32, ptr %..., align 1` per-lane loads, all bare.

## "Extending case" / alignment loss

Independently of metadata: at line 731,
```cpp
const Align AdjustedAlignment = commonAlignment(Alignment, EltTy->getPrimitiveSizeInBits()/8);
```
This is applied **uniformly** to all per-lane loads, including the very first
one which loads from `Ptr` directly (line 806 in the dynamic path; line 748
when Idx == 0 in the constant path).

For a `<4 x i32>` expandload with `align 16` on the base ptr, the first lane
load can keep `align 16` — the GEP for it is offset 0. Instead all loads get
`align 4`, including the head one (`/tmp/w360/expandload-aligned.ll` confirms
`align 4` on the head load). This is a separate missed-opt that bundles
naturally with the metadata fix because both touch the same `CreateAlignedLoad`
sites.

## Where to fix

- Line 749 (constant mask): `cast<LoadInst>(InsertElt)->copyMetadata(*CI)`.
- Line 806 (dynamic mask): `Load->copyMetadata(*CI)`.
- Optional alignment improvement: for `Idx == 0` use the original `Alignment`
  (which is already the masked.expandload's pointer attribute), not the
  per-element `AdjustedAlignment`.

## Triage notes

Independent of w104 (which only mentioned load/store/gather/scatter constant
paths). Expandload is qualitatively different because it has no all-true /
splat short-cut, so EVERY use of expandload that goes through this pass loses
metadata, regardless of mask form.
