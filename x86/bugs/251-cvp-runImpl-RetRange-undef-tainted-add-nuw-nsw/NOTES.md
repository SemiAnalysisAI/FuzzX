# 251 — CVP infers `range` return-attr AND `add nuw nsw` from `select i1 %cmp, i64 undef, i64 1` — Alive2-falsifiable

Component: `llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp` `runImpl` (return range inference) AND `processBinOp` (nsw/nuw inference)

Root cause in `llvm/include/llvm/Analysis/ValueLattice.h:247-249,282-290` — `ValueLatticeElement::asConstantRange(/*UndefAllowed=*/false)` short-circuits via the `Range.isSingleElement()` clause and strips the "may-be-undef" marker. CVP then treats the operand's lattice (a `constantrange_including_undef` with single element `{1}`) as a hard fact.

With `%cmp = 1`:
- Source `add undef, 1` can be any `i64` (undef refines to any value).
- Target `add nuw nsw undef, 1` becomes **poison** for many `undef` choices (e.g., `INT_MAX`).
- Plus `range(i64 1, 3)` further refines any non-`{1,2}` choice to poison.

Same root cause as upstream issue #114902 (open since 2024-11-05); this candidate additionally documents the `add nuw nsw` path.

## Reproducer

```ll
define i64 @pr64669_2(i1 %cmp) {
  %mul = select i1 %cmp, i64 undef, i64 1
  %conv3 = zext i1 %cmp to i64
  %add = add i64 %mul, %conv3
  ret i64 %add
}
```

`opt -passes=correlated-propagation -S` →
```
define range(i64 1, 3) i64 @pr64669_2(i1 %cmp) {
  %mul = select i1 %cmp, i64 undef, i64 1
  %add = add nuw nsw i64 %mul, %conv3
  ret i64 %add
}
```

Both the return attr and the nuw/nsw on the add are unsound.

## Severity

Default x86 -O2 (CVP runs in default O2). Real Alive2-falsifiable miscompile. Already reported upstream as #114902.

## Fix

`asConstantRange` should not strip the may-be-undef marker via the single-element short-cut — or callers in CVP must pass `UndefAllowed=true` when consuming the result.
