# w104: ScalarizeMaskedMemIntrin drops !range/!nontemporal/!noalias on constant-mask fast path

## Status: confirmed (observable downstream missed-opt / potential miscompile vehicle)

## Summary

`scalarizeMaskedLoad`, `scalarizeMaskedStore`, `scalarizeMaskedGather` and
`scalarizeMaskedScatter` all have a "constant mask" fast path that lowers a
`llvm.masked.{load,store,gather,scatter}` directly into scalar
load/insertelement (or extract/store) sequences.  Those scalar loads/stores
are created with `IRBuilder::CreateAlignedLoad`/`CreateAlignedStore` and
**never receive any metadata from the original intrinsic call**.

Compare with the all-true mask case (line 167-169), which correctly calls
`NewI->copyMetadata(*CI)`.  The constant-mask (but not all-true) path at
lines 184-194 and the gather/scatter constant-mask paths at lines 493-506 do
not propagate any metadata from the original intrinsic.

Metadata silently dropped includes `!range`, `!nontemporal`,
`!invariant.load`, `!noalias`, `!alias.scope`, `!nonnull`,
`!dereferenceable`, `!dereferenceable_or_null`, `!align`, `!noundef`,
`!annotation`, `!mmra`, `!fpmath`, plus any TBAA.

`!range` and `!noundef` carry semantic guarantees that downstream passes
rely on for *correct* folding.  In the constant-mask path we erase those
guarantees, so the scalarized program permits behavior that the original
intrinsic call did not.  In the reverse direction (which is the only
direction LLVM cares about for correctness) this is "safe" because dropping
a guarantee can only *forbid* downstream folds — but the test below shows a
clear, reproducible observable regression in `instcombine`, and the same
mechanism is the textbook vehicle for full miscompiles when paired with a
later pass that re-derives the dropped guarantee from a different source.

## Reproducer

`/tmp/sc_e2e.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
define i1 @test_range_loss(ptr %p, <4 x i32> %src) {
  %v = call <4 x i32> @llvm.masked.load.v4i32.p0(
        ptr %p, i32 16,
        <4 x i1> <i1 true, i1 false, i1 false, i1 false>,
        <4 x i32> %src), !range !0
  %e = extractelement <4 x i32> %v, i32 0
  %c = icmp uge i32 %e, 2
  ret i1 %c
}
declare <4 x i32> @llvm.masked.load.v4i32.p0(ptr, i32, <4 x i1>, <4 x i32>)
!0 = !{i32 0, i32 2}
```

### Without scalarize (baseline, instcombine only)

```
$ opt -passes=instcombine -mtriple=x86_64-- -S /tmp/sc_e2e.ll
define i1 @test_range_loss(ptr %p, <4 x i32> %src) {
  ret i1 false
}
```

`!range [0,2)` lets instcombine fold `icmp uge %v, 2` to `false`.

### With scalarize-masked-mem-intrin + instcombine

```
$ opt -passes='scalarize-masked-mem-intrin,instcombine' -mtriple=x86_64-- -S /tmp/sc_e2e.ll
define i1 @test_range_loss(ptr %p, <4 x i32> %src) {
  %1 = load i32, ptr %p, align 4          ; <-- no !range
  %c = icmp ugt i32 %1, 1
  ret i1 %c
}
```

The scalar `load` carries no `!range`, instcombine cannot fold the icmp,
and the program now performs an avoidable comparison + branch.

## Where to fix

`llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp`:

- `scalarizeMaskedLoad`, constant-mask path, line 188 — add
  `Load->copyMetadata(*CI)` after the `CreateAlignedLoad`.
- `scalarizeMaskedStore`, constant-mask path, line 355 — add metadata copy
  to the new `Store`.
- `scalarizeMaskedGather`, constant-mask path, line 499 — add metadata copy
  to each new `Load`.
- `scalarizeMaskedScatter`, constant-mask path, line 638 — add metadata
  copy to each new `Store`.

The existing dynamic-mask paths use `copyMetadata` only on a single
representative load/store, which is also dubious but at least preserves the
metadata on one access.  The fast paths preserve nothing.

## Triage notes for parent

This is a missed-opt with a direct reproducer, not a wrong-codegen on its
own.  However it is the *generic* metadata-loss class bug: any pass that
later relies on `!range`, `!noalias`, `!noundef`, `!nonnull`,
`!dereferenceable`, or `!invariant.load` to justify a *transform* now sees
an unannotated load.  Dropping is always safe in the OPT->LESS-OPT
direction; if a downstream pass *moves* the guarantee from the deleted call
back onto the scalar load via a different mechanism (e.g. attributor) the
dropping is harmless.  But this should still be fixed.
