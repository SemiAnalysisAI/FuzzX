# CVP `runImpl` infers `range` attr and `add nsw/nuw` from a `select`/`phi` value whose lattice includes `undef`, refining undef into poison

## File and root cause

`llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp`

Two cooperating paths combine to produce a strict refinement of a defined
value into poison:

1. **Return-value range attribute inference** in `runImpl`
   (lines 1338-1357, 1362-1373):

```cpp
// llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp:1343-1356
auto *RetVal = RI->getReturnValue();
if (!RetVal) break; // handle "ret void"
if (RetRange && !RetRange->isFullSet())
  RetRange =
      RetRange->unionWith(LVI->getConstantRange(RetVal, RI,
                                                /*UndefAllowed=*/false));
// ...
// lines 1362-1373:
if (RetRange && !RetRange->isFullSet()) {
  Attribute RangeAttr = F.getRetAttribute(Attribute::Range);
  if (RangeAttr.isValid())
    RetRange = RetRange->intersectWith(RangeAttr.getRange());
  if (!RetRange->isEmptySet() && !RetRange->isSingleElement()) {
    F.addRangeRetAttr(*RetRange);
    FnChanged = true;
  }
}
```

2. **`processBinOp`** no-wrap inference (lines 1179-1211):

```cpp
// llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp:1188-1208
ConstantRange LRange = LVI->getConstantRangeAtUse(BinOp->getOperandUse(0),
                                                  /*UndefAllowed=*/false);
ConstantRange RRange = LVI->getConstantRangeAtUse(BinOp->getOperandUse(1),
                                                  /*UndefAllowed=*/false);
// ...
if (!NUW) {
  ConstantRange NUWRange = ConstantRange::makeGuaranteedNoWrapRegion(
      Opcode, RRange, OBO::NoUnsignedWrap);
  NewNUW = NUWRange.contains(LRange);
  ...
}
if (!NSW) {
  ConstantRange NSWRange = ConstantRange::makeGuaranteedNoWrapRegion(
      Opcode, RRange, OBO::NoSignedWrap);
  NewNSW = NSWRange.contains(LRange);
  ...
}
setDeducedOverflowingFlags(BinOp, Opcode, NewNSW, NewNUW);
```

Both call sites pass `UndefAllowed=false` and treat the returned
`ConstantRange` as a hard fact. The problem is in
`ValueLatticeElement::asConstantRange`
(`llvm/include/llvm/Analysis/ValueLattice.h:282-290`):

```cpp
// llvm/include/llvm/Analysis/ValueLattice.h:247-249, 282-290
bool isConstantRange(bool UndefAllowed = true) const {
  return Tag == constantrange || (Tag == constantrange_including_undef &&
                                  (UndefAllowed || Range.isSingleElement()));
}
// ...
ConstantRange asConstantRange(unsigned BW, bool UndefAllowed = false) const {
  if (isConstantRange(UndefAllowed))
    return getConstantRange();
  // ...
  return ConstantRange::getFull(BW);
}
```

When the lattice element is `constantrange_including_undef` and the
range is a **single element**, `isConstantRange(/*UndefAllowed=*/false)`
returns `true` and the range is returned **stripped of the
"may-be-undef" flag**. CVP then treats the single-element range as a
proven range, but at runtime the value can legitimately be any value
(the `undef` half of the merge).

This is the same root cause as upstream issue
[#114902](https://github.com/llvm/llvm-project/issues/114902)
(opened 2024-11-05, still **open** and unfixed as of 2026-05-21). The
issue lists only the `range`-attribute path; the `add nsw nuw` path is
the same bug, also visible on the reproducer below.

## Reproducer

`x86/candidates/w620-cvp-runImpl-RetRange-undef-tainted-add-nuw-nsw.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i64 @pr64669_2(i1 %cmp) {
  %mul = select i1 %cmp, i64 undef, i64 1
  %conv3 = zext i1 %cmp to i64
  %add = add i64 %mul, %conv3
  ret i64 %add
}
```

### `opt -passes=correlated-propagation -S` diff

Before:
```llvm
define i64 @pr64669_2(i1 %cmp) {
  %mul = select i1 %cmp, i64 undef, i64 1
  %conv3 = zext i1 %cmp to i64
  %add = add i64 %mul, %conv3
  ret i64 %add
}
```

After (observed):
```llvm
define range(i64 1, 3) i64 @pr64669_2(i1 %cmp) {
  %mul = select i1 %cmp, i64 undef, i64 1
  %conv3 = zext i1 %cmp to i64
  %add = add nuw nsw i64 %mul, %conv3
  ret i64 %add
}
```

Both the `range(i64 1, 3)` return-value attribute **and** the
`nuw nsw` flags on the `add` are incorrect:

* **Source semantics with `%cmp = 1`**: `%mul = undef` (can be any
  `i64`, e.g. `4`); `%conv3 = 1`; `%add = undef + 1`. The frontend
  freely picks `%add = 1` to make the program well-defined, so the
  source returns a defined `i64` that lies in any range the picker
  chooses.

* **Target semantics with `%cmp = 1`**: pick `undef = 4`. Then
  `%add = add nuw nsw 4, 1 = 5`. Since `5 ∉ [1, 3)`, the return-value
  `range` attribute is violated and the call returns **poison**.

  Independently, pick `undef = 0x7FFF_FFFF_FFFF_FFFF` (`INT64_MAX`).
  Then `add nsw 0x7FFF_FFFF_FFFF_FFFF, 1` is **poison** because of
  signed overflow.

So CVP refined a value-that-could-be-chosen-defined into a
guaranteed-poison value (UB on the call site if `noundef`, miscompile
if the return is later used). Alive2 verdict from the upstream issue:
*"Target is more poisonous than source."*

## Why both flags get set

Trace for the `add`:

1. `processBinOp` queries the operand uses.
2. For `%mul`'s operand-use in the `add`, `getConstantRangeAtUse` walks
   into `solveBlockValueSelect`
   (`llvm/lib/Analysis/LazyValueInfo.cpp:911-993`), which computes
   `TrueVal = undef`, `FalseVal = constantrange [1, 2)`, then
   `TrueVal.mergeIn(FalseVal)` produces lattice tag
   `constantrange_including_undef` with `Range = [1, 2)`.
3. `asConstantRange(_, /*UndefAllowed=*/false)` short-circuits via the
   `Range.isSingleElement()` clause and returns `[1, 2)` instead of
   the full range — losing the "may be undef" flag.
4. With `LRange = [1, 2)` and `RRange = [0, 2)` (the `zext i1`),
   `makeGuaranteedNoWrapRegion(Add, [0, 2), NUW)` and
   `(Add, [0, 2), NSW)` both contain `[1, 2)`, so both `NewNUW` and
   `NewNSW` fire.
5. `setDeducedOverflowingFlags` sets `nsw` and `nuw` on the `add`.

The return-attribute path is the same: `LVI->getConstantRange(%add, RI,
false)` ends up consulting the same single-element-but-undef-tainted
lattice element and unions it (still as a clean range) into `RetRange`,
which then becomes `[1, 3)` (`mul = 1`, `conv = 0 or 1`, `add = 1 or
2`).

## Fix sketch (matches upstream issue suggestion)

Two interchangeable fixes:

* **Tighten `ValueLatticeElement::asConstantRange`** so that
  `UndefAllowed=false` never returns a range from
  `constantrange_including_undef`, even for single elements.
  `getValueLatticeElement::asConstantInteger` is the right pattern for
  the single-element/constant case — callers that want a constant
  should use that and not `asConstantRange(/*UndefAllowed=*/false)`.

* **Or filter at every CVP call site** that consumes the range as a
  no-poison fact: in addition to checking `Range`, query the lattice
  element directly (e.g. through a new
  `LVI->getValueLatticeElement(...)` wrapper) and skip the transform
  when the tag is `constantrange_including_undef`.

The minimal-surface fix for `runImpl`'s range-attr inference is to
abandon the union and bail out early when any `RetVal` traces to an
include-undef lattice element:

```cpp
ValueLatticeElement VL = LVI->getValueLatticeElement(RetVal, RI);
if (VL.isConstantRangeIncludingUndef())
  RetRange.reset();   // give up; we cannot add a range attr soundly
else
  RetRange = RetRange->unionWith(VL.asConstantRange(...));
```

Same shape for `processBinOp` (`add nsw/nuw` deduction), `processTrunc`
(`trunc nsw/nuw`), `processSExt` (`sext` -> `zext nneg`),
`processSIToFP` (`sitofp` -> `uitofp nneg`), `processPossibleNonNeg`
(adds `nneg` to `zext`/`uitofp`), and any other site that uses the
range to attach a poison-producing flag.

## Notes on scope

The same bug appears for `phi`-of-undef-and-constant patterns
(`solveBlockValueOverwrittenPHI` produces the same
`constantrange_including_undef` tag) — the reproducer above uses
`select` because it is the most compact form.

Upstream issue
[#114902](https://github.com/llvm/llvm-project/issues/114902) is open
since 2024-11-05; this candidate documents the same defect plus the
*previously unlisted* `add nsw nuw` second path triggered by the same
single-element-undef lattice quirk.
