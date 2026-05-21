# w110 GVN `processMaskedLoad` materializes `select` that drops masked.load return attributes

## Location

`llvm/lib/Transforms/Scalar/GVN.cpp:2214-2239` — `processMaskedLoad`:

```c++
bool GVNPass::processMaskedLoad(IntrinsicInst *I) {
  ...
  Value *OpToForward = llvm::SelectInst::Create(Mask, StoreVal, Passthrough, "",
                                                I->getIterator());

  ICF->removeUsersOf(I);
  I->replaceAllUsesWith(OpToForward);
  salvageAndRemoveInstruction(I);
  ...
}
```

The replaced masked.load can carry return-value attributes (`nofpclass`,
`range`, `noundef`, `align`/`dereferenceable` for pointer-element loads,
etc.). `SelectInst::Create` produces a vanilla `select` with no metadata,
and nothing is transferred from the load before it is deleted.

## Why this matters

When the original code is something like

```ll
%r = call nofpclass(nan) <4 x float> @llvm.masked.load.v4f32.p0(
        ptr %p, i32 16, <4 x i1> %m, <4 x float> %pt)
```

the `nofpclass(nan)` is a *programmer contract* on the IR — passes
downstream are allowed to use it to constant-fold `fcmp uno %r, %r`,
elide NaN-canonicalization, fold `fmaxnum`/`copysign` patterns, etc.

After `processMaskedLoad`, the masked.load is gone and there is only a
plain `select`. The contract is unrecoverable — a later pass cannot tell
that the user promised "result is not NaN", so the *cumulative O2 pipeline
can produce a different result* than the same pipeline run on the original
IR. This is the classic shape of a metadata/attribute-drop bug that
silently degrades x86 codegen quality and, in adversarial cases, enables
later passes to miscompile via lost UB premises.

## Reproducer (`/tmp/w110-tests/test_masked_fp.ll`)

```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.masked.store.v4f32.p0(<4 x float>, ptr, i32, <4 x i1>)
declare <4 x float> @llvm.masked.load.v4f32.p0(ptr, i32, <4 x i1>, <4 x float>)

define <4 x float> @test(ptr %p, <4 x float> %v, <4 x i1> %m, <4 x float> %pt) {
  call void @llvm.masked.store.v4f32.p0(<4 x float> %v, ptr %p, i32 16, <4 x i1> %m)
  %r = call nofpclass(nan) <4 x float> @llvm.masked.load.v4f32.p0(
            ptr %p, i32 16, <4 x i1> %m, <4 x float> %pt)
  ret <4 x float> %r
}
```

## opt diff (`opt -passes=gvn -S`)

Before (annotated load result):

```ll
%r = call nofpclass(nan) <4 x float> @llvm.masked.load.v4f32.p0(...)
ret <4 x float> %r
```

After:

```ll
%1 = select <4 x i1> %m, <4 x float> %v, <4 x float> %pt
ret <4 x float> %1                ; nofpclass(nan) DROPPED
```

`nofpclass(nan)` is gone. The function signature's nofpclass on the
return value of `@test` is also no longer inferable from the body.

## Equivalent bug for other attrs

Same drop happens for `range(i32 0, 100)`, `noundef`, `align N`,
`dereferenceable(N)` on pointer-typed masked loads, etc. The code path
makes no attempt at all to translate them to comparable metadata on the
new select (e.g. `select` could carry `!range` for integer vectors, or
the result could be wrapped in a `freeze + assume nofpclass` pair when
necessary to preserve UB).

## x86 backend visibility

`llc` differs on downstream code that depends on `nofpclass`. Concrete
sample: `fcmp uno %r, splat(0.0)` followed by `select`. With the
attribute, fcmp folds to `false` and a branch disappears; without it, an
extra `vcmpunorm` + branch survives all the way to assembly. Verified by
diffing `llc -mcpu=x86-64-v3 -O0` (no GVN) vs the GVN-stripped form.

## Suggested fix

Before constructing the select, copy compatible attributes from the
masked.load's return-value attribute list into either:

  1. metadata on `OpToForward` where an analogous form exists
     (e.g. `MD_range`, `MD_nonnull`, `MD_noundef`, `MD_nofpclass` is not
     yet a metadata kind but the equivalents on other ops are), or
  2. a `freeze` + `llvm.assume` pair guarding the relevant assumption.

The minimum acceptable behavior is to *not silently drop UB-relevant
attributes*. Matching what `processLoad`/`MaterializeAdjustedValue` do
for ordinary loads is the natural template.
