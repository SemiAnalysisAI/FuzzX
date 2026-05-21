# w301: canonicalizeCmpWithConstant silently drops `samesign` flag

## Location
`amdgpu/third_party/llvm-project/llvm/lib/Transforms/InstCombine/InstCombineCompares.cpp:7232-7249`

## Issue (information loss, not a miscompile)
```cpp
static ICmpInst *canonicalizeCmpWithConstant(ICmpInst &I) {
  ICmpInst::Predicate Pred = I.getPredicate();   // <-- drops samesign here
  ...
  auto FlippedStrictness = getFlippedStrictnessPredicateAndConstant(Pred, Op1C);
  if (!FlippedStrictness)
    return nullptr;
  return new ICmpInst(FlippedStrictness->first, Op0, FlippedStrictness->second);
}
```
`Pred` is fetched as `CmpInst::Predicate`, dropping the `samesign` bit.
`getFlippedStrictnessPredicateAndConstant` returns a `Predicate` (not
`CmpPredicate`), and the new `ICmpInst` is constructed without the flag.

Concrete observation:
```ll
define i1 @f(i32 %x) {
  %c = icmp samesign ule i32 %x, 5
  ret i1 %c
}
```
After `opt -passes=instcombine -S` becomes:
```ll
define i1 @f(i32 %x) {
  %c = icmp ult i32 %x, 6   ; samesign dropped
  ret i1 %c
}
```

## Why this is "only" information loss
`samesign ule X, 5` is poison when `X` is negative; `ult X, 6` is `false`
for negative X. Replacing poison with `false` is a valid refinement, so this
is not a miscompile. But downstream analyses lose the implication
"if this is true, X is non-negative", which `samesign` was encoding.

## Fix sketch
Read `I.getCmpPredicate()`, propagate `hasSameSign()` into the new instruction
via `I->setSameSign(true)` after construction (or refactor
`getFlippedStrictnessPredicateAndConstant` to return a `CmpPredicate`).
Note that flipping `ule` <-> `ult` preserves the samesign property because
the same operands are compared; only the constant changes by ±1, which
cannot cross zero from the `ConstantIsOk` guard inside
`getFlippedStrictnessPredicateAndConstant`.

## Notes
- Severity: missed-opt only. No miscompile produced.
- Same shape of bug exists at the other two callers of
  `getFlippedStrictnessPredicateAndConstant` inside
  `InstCombineCompares.cpp` (lines 2483 and 3335) — the caller drops the
  samesign bit before passing the predicate in. Worth a sweep of all
  call-sites if a real fix lands.
