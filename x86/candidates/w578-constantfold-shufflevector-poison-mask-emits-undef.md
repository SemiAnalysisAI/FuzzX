# ConstantFoldShuffleVectorInstruction: poison mask element emits undef instead of poison

## Summary

`ConstantFoldShuffleVectorInstruction` in `llvm/lib/IR/ConstantFold.cpp`
folds a constant `shufflevector` element-by-element.  When a mask
element is the `PoisonMaskElem` sentinel (`-1`, representing both undef
and poison mask elements), the code emits `UndefValue::get(EltTy)` for
that lane.  LangRef explicitly requires `PoisonValue` instead:

> A ``poison`` element in the mask vector specifies that the resulting
> element is ``poison``. For backwards-compatibility reasons, LLVM
> temporarily also accepts ``undef`` mask elements. These will be
> interpreted the same way as ``poison`` mask elements, also producing
> a ``poison`` element in the result.
>
> -- LangRef, shufflevector semantics, lines 11659-11662

So both undef and poison mask elements must produce **poison** in the
result, not undef.  The fold currently produces undef, weakening the
poison.

This matters because `poison` propagates much more aggressively than
`undef` (e.g., `poison & 0` is still `poison`, `poison + 0` is still
`poison`, branching on `poison` is UB).  Replacing poison with undef
silently loses these guarantees, and downstream code can treat the
weakened value as a normal undef (e.g., `i32 undef & 0 -> 0`).

## Source

`llvm/lib/IR/ConstantFold.cpp:479-536`
(`ConstantFoldShuffleVectorInstruction`):

```cpp
479 Constant *llvm::ConstantFoldShuffleVectorInstruction(Constant *V1, Constant *V2,
480                                                      ArrayRef<int> Mask) {
...
512   // Loop over the shuffle mask, evaluating each element.
513   SmallVector<Constant*, 32> Result;
514   for (unsigned i = 0; i != MaskNumElts; ++i) {
515     int Elt = Mask[i];
516     if (Elt == -1) {
517       Result.push_back(UndefValue::get(EltTy));   // <-- should be PoisonValue
518       continue;
519     }
...
521     if (unsigned(Elt) >= SrcNumElts*2)
522       InElt = UndefValue::get(EltTy);             // <-- arguably the same
...
535   return ConstantVector::get(Result);
536 }
```

Line 517 covers both an explicit `i32 poison` and an explicit `i32
undef` mask element (both encoded as the sentinel `-1` /
`PoisonMaskElem`).  LangRef says both should produce `poison`.

Compare the all-poison-mask shortcut at line 488-490 which *does*
return PoisonValue:

```cpp
487   // Poison shuffle mask -> poison value.
488   if (all_of(Mask, equal_to(PoisonMaskElem))) {
489     return PoisonValue::get(VectorType::get(EltTy, MaskEltCount));
490   }
```

The per-element path (line 517) is inconsistent with that shortcut.

## Reproducer (x86-64, default `-O2` pipeline)

`shuffle_mask_poison.ll`:

```llvm
; constant shufflevector with explicit poison mask element
define <4 x i32> @shuf_poison_mask_elt() {
  ret <4 x i32> shufflevector (<4 x i32> <i32 1, i32 2, i32 3, i32 4>,
                               <4 x i32> <i32 5, i32 6, i32 7, i32 8>,
                               <4 x i32> <i32 0, i32 poison, i32 4, i32 7>)
}

; constant shufflevector with explicit undef mask element
; per LangRef this must be treated as poison too
define <4 x i32> @shuf_undef_mask_elt() {
  ret <4 x i32> shufflevector (<4 x i32> <i32 1, i32 2, i32 3, i32 4>,
                               <4 x i32> <i32 5, i32 6, i32 7, i32 8>,
                               <4 x i32> <i32 0, i32 undef, i32 4, i32 7>)
}
```

```
$ opt -passes=instcombine -S shuffle_mask_poison.ll
define <4 x i32> @shuf_poison_mask_elt() {
  ret <4 x i32> <i32 1, i32 undef, i32 5, i32 8>     ; lane 1 should be poison
}
define <4 x i32> @shuf_undef_mask_elt() {
  ret <4 x i32> <i32 1, i32 undef, i32 5, i32 8>     ; lane 1 should be poison
}
```

For comparison, `opt -S` (no passes) preserves the literal `poison` in
the mask; only the constant folder rewrites it to `undef`.

## Fix sketch

Replace `UndefValue::get(EltTy)` with `PoisonValue::get(EltTy)` at
line 517:

```cpp
514   for (unsigned i = 0; i != MaskNumElts; ++i) {
515     int Elt = Mask[i];
516     if (Elt == -1) {
-       Result.push_back(UndefValue::get(EltTy));
+       Result.push_back(PoisonValue::get(EltTy));
518       continue;
519     }
```

This brings the per-element path in line with the all-poison-mask
shortcut already in place at line 488-490 and with LangRef.

The out-of-range `Elt >= SrcNumElts*2` case at line 522 deserves a
similar audit; mask elements are required by LangRef to be `i32
ConstantInt` or `poison`, so out-of-range values are not really
expected.  But if such a mask is somehow constructed (via legacy IR
generators or unchecked APIs), the existing `UndefValue` is also
weaker than necessary.

## Why this matters at -O2

- `shufflevector` with poison/undef mask elements appears in many
  patterns: the result of `shufflevector` produced by
  vectorization, by IR-level lane-zeroing idioms, by manual SIMD code
  using `__builtin_shuffle`, and by codegen-friendly canonicalizations
  that mark unused lanes as poison.
- After this fold turns the poison lane into `undef`, downstream
  passes (InstCombine itself, ValueTracking, GVN, vectorization) can
  legitimately reason about the `undef` lane as "any value" and pick
  arms / fold compares / hoist around it in ways that they could not
  have done if the lane were `poison`.
- The InstCombine call to ConstantFoldConstant runs eagerly on every
  ConstantExpr, so the moment such a `shufflevector` constant appears
  as an operand of any instruction in the function, the poison gets
  silently demoted to undef.
