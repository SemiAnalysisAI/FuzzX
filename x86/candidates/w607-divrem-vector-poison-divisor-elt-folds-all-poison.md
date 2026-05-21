# w607 — `commonIDivRemTransforms` value-wise poison fold over per-element vector divrem

## Target
- `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:1291-1303`
- Within `InstCombinerImpl::commonIDivRemTransforms(BinaryOperator &I)`.

## Mechanism

```cpp
// If any element of a constant divisor fixed width vector is zero or undef
// the behavior is undefined and we can fold the whole op to poison.
auto *Op1C = dyn_cast<Constant>(Op1);
Type *Ty = I.getType();
auto *VTy = dyn_cast<FixedVectorType>(Ty);
if (Op1C && VTy) {
  unsigned NumElts = VTy->getNumElements();
  for (unsigned i = 0; i != NumElts; ++i) {
    Constant *Elt = Op1C->getAggregateElement(i);
    if (Elt && (Elt->isNullValue() || isa<UndefValue>(Elt)))
      return replaceInstUsesWith(I, PoisonValue::get(Ty));
  }
}
```

`isa<UndefValue>(Elt)` is true for both `undef` and `poison` element
constants (`PoisonValue` extends `UndefValue`). So any vector divrem with a
constant divisor whose vector has any `0`, `undef`, or `poison` lane is
replaced with `poison` for the whole vector.

This is a **value-wise** poison rule: a single poisoned (or zero) divisor
lane contaminates *all* lanes of the result, including lanes whose divisor
is a perfectly well-defined nonzero constant.

## .ll repro

```llvm
; opt -passes=instcombine -S

; Element 0 is "X[0] urem -5" — perfectly defined for any concrete X[0].
; Element 1's divisor is poison, so element 1 of the urem is poison.
; Extracting element 0 should give the well-defined element 0 result.
define i32 @extract_well_defined_lane(<2 x i32> %x) {
  %r = urem <2 x i32> %x, <i32 -5, i32 poison>
  %e = extractelement <2 x i32> %r, i64 0
  ret i32 %e
}
```

Locked in by `llvm/test/Transforms/InstCombine/vector-urem.ll:74-80`
(`test_v4i32_negconst_poison` expects `ret <4 x i32> poison`) and
`vector-urem.ll:22-28` (`test_v4i32_const_pow2_poison` expects the
same).

## opt diff

```
define i32 @extract_well_defined_lane(<2 x i32> %x) {
-  %r = urem <2 x i32> %x, <i32 -5, i32 poison>
-  %e = extractelement <2 x i32> %r, i64 0
-  ret i32 %e
+  ret i32 poison
}
```

Element 0 of the original is `%x[0] urem -5` (an unsigned remainder by a
constant > 0 unsigned), a perfectly defined value for any concrete `%x[0]`.
After instcombine, element 0 has been replaced by `poison`.

## Discussion

This is a **deliberate** transform (per the in-source comment) and a
**locked-in** behavior (covered by upstream `vector-urem.ll` tests). It
also matches the analogous handling in `vector-srem.ll`,
`vector-udiv.ll`, and `vector-sdiv.ll`. The justification given is that
"the behavior is undefined" — i.e., LLVM has chosen value-wise (rather
than per-element) UB propagation for vector div/rem.

However:
- LangRef is ambiguous on per-element vector poison semantics — recent
  discussion (e.g.
  [https://discourse.llvm.org/t/documentation-of-detailed-poison-semantics/81243](https://discourse.llvm.org/t/documentation-of-detailed-poison-semantics/81243))
  and the PLDI 2026 work on removing `undef` argue that per-element is
  the more defensible semantics.
- An asymmetry exists within the same test file:
  `vector-urem.ll:test_v4i32_one_poison` (`urem <1,1,1,poison>, %a0` — poison
  in the **dividend**) keeps per-element semantics
  (`zext (icmp ne %a0, 1)`), while
  `test_v4i32_negconst_poison` (poison in the **divisor**) collapses to
  all-poison.
- The dividend-poison case is sound under per-element semantics (poison
  dividend produces poison only in that lane). The divisor-poison fold
  produces a strictly less-defined result (well-defined lanes turn into
  poison), which is a textbook *refinement violation* under per-element
  semantics.

If LLVM's intended semantics is value-wise UB propagation across vector
lanes (matching the in-source comment), this should be documented in
LangRef. If LangRef is read as per-element (the natural reading for
arithmetic vectors), this fold is unsound: it replaces a `urem` whose
element 0 is `%x[0] urem -5` with `poison`, losing information.

## Severity
Low. Locked in by tests; clearly intentional. Filing as a candidate
because the per-element vs value-wise question is an open LLVM design
issue and this fold sits squarely on the boundary. If LangRef ever
clarifies in favor of per-element semantics, this transform becomes a
refinement violation and would need to be replaced with a fold that
preserves the well-defined lanes (e.g. keep the original urem, or
constant-fold only the well-defined lanes).
