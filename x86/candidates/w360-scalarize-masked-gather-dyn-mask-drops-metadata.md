# w360: ScalarizeMaskedMemIntrin scalarizeMaskedGather (dynamic-mask) drops ALL metadata on per-lane loads

## Status: confirmed (reproducer; downstream observable miscompile-vehicle; complementary to w104/#180)

## Where (source:lines)

`llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp`:
- `scalarizeMaskedGather`, dynamic-mask loop, line **557-558**: `LoadInst *Load = Builder.CreateAlignedLoad(EltTy, Ptr, AlignVal, "Load" + Twine(Idx));` is followed immediately by `Builder.CreateInsertElement(...)` with **no `Load->copyMetadata(*CI)` call**.

The only `copyMetadata` calls in the whole file are at lines 168, 211, 339, 376 — all for `scalarizeMaskedLoad`/`scalarizeMaskedStore`. None for gather/scatter/expandload/compressstore/histogram.

## How this differs from w104/#180

w104 covered only the **constant-mask** fast paths (lines 184-194, 350-359, 493-506, 631-642). That is one of two metadata-loss paths per intrinsic. This candidate (w360) covers the **non-constant-mask** path of `scalarizeMaskedGather` (lines 517-574), which is the path actually triggered by the typical lowering of dynamic SIMD code (e.g. vectorized loops with runtime masks). That path also drops every piece of metadata — and it is the path that produces *N* scalar loads (one per lane), each unannotated.

## Reproducer (downstream observable: `!range` -> instcombine cannot fold)

`/tmp/w360/gather-dyn-rangefold.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
define i1 @gather_dyn_range_fold(<4 x ptr> %ptrs, <4 x i1> %m, <4 x i32> %src) {
  %v = call <4 x i32> @llvm.masked.gather.v4i32.v4p0(
        <4 x ptr> %ptrs, i32 4, <4 x i1> %m,
        <4 x i32> <i32 0, i32 0, i32 0, i32 0>), !range !0
  %e = extractelement <4 x i32> %v, i32 0
  %c = icmp uge i32 %e, 2
  ret i1 %c
}
declare <4 x i32> @llvm.masked.gather.v4i32.v4p0(<4 x ptr>, i32, <4 x i1>, <4 x i32>)
!0 = !{i32 0, i32 2}
```

### Baseline (instcombine alone — gather kept intact)

```
$ opt -passes=instcombine -mtriple=x86_64-- -S /tmp/w360/gather-dyn-rangefold.ll
define i1 @gather_dyn_range_fold(<4 x ptr> %ptrs, <4 x i1> %m, <4 x i32> %src) {
  ret i1 false
}
```

`!range [0,2)` on the gather lets instcombine fold `icmp uge %v, 2` to `false`.

### With scalarize-masked-mem-intrin in front (per-lane loads stripped of `!range`)

```
$ opt -passes='scalarize-masked-mem-intrin,instcombine' -mtriple=x86_64-- -S /tmp/w360/gather-dyn-rangefold.ll
... 4 conditional blocks, each with:
  %LoadN = load i32, ptr %PtrN, align 4    ; <-- NO !range
...
  %c = icmp ugt i32 %e, 1
  ret i1 %c
```

Same observable mechanic as w104, but on the dynamic-mask path that is hit at runtime on common SIMD loops. After scalarization, instcombine cannot prove the result is `< 2`, so the avoidable compare/branch stays in the function.

## Other metadata silently dropped on the per-lane loads

`!nontemporal`, `!noalias`, `!alias.scope`, `!nonnull`, `!dereferenceable`,
`!dereferenceable_or_null`, `!align`, `!noundef`, `!invariant.load`,
`!invariant.group`, `!annotation`, `!mmra`, `!fpmath` (where applicable),
TBAA — none of these propagate.

## Where to fix

After line 558 add:
```cpp
Load->copyMetadata(*CI);
```

(With the per-load caveat: `!nontemporal` and `!noalias`/`!alias.scope` propagate as-is to each lane load; `!range`/`!nonnull`/`!dereferenceable` apply to the gathered scalar value of THAT lane — exactly the per-lane load. So per-lane copy is the right semantics.)

## Triage notes for parent

Closely related to w104/#180 but distinct: w104 is the constant-mask fast path; this is the variable-mask path producing one per-lane scalar load per element. Both should be fixed together — only fixing the constant-mask path leaves the more common runtime case unhandled.
