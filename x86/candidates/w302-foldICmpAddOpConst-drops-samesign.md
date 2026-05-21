# w302: foldICmpAddOpConst drops `samesign` on (X+Cst) cmp X overflow check

## Location
`amdgpu/third_party/llvm-project/llvm/lib/Transforms/InstCombine/InstCombineCompares.cpp:912-957`
called from `:7622-7627`

## Issue (info loss, not a miscompile)
The function `foldICmpAddOpConst(Value *X, const APInt &C, CmpPredicate Pred)`
receives `Pred` as `CmpPredicate` (so the samesign bit is preserved across
the call), but then constructs the output instruction with only the raw
`Predicate`:
```cpp
if (Pred == ICmpInst::ICMP_ULT || Pred == ICmpInst::ICMP_ULE) {
  Constant *R =
      ConstantInt::get(X->getType(), APInt::getMaxValue(C.getBitWidth()) - C);
  return new ICmpInst(ICmpInst::ICMP_UGT, X, R);   // <-- samesign dropped
}
```
The `==` operator on `CmpPredicate` compares only the underlying predicate
(per `IR/CmpPredicate.h:68`), so the samesign bit is silently lost when the
caller passed it in.

## Concrete observation
```ll
define i1 @f(i8 %x) {
  %a = add i8 %x, 1
  %c = icmp samesign ult i8 %a, %x
  ret i1 %c
}
```
After `opt -passes=instcombine -S`:
```ll
define i1 @f(i8 %x) {
  %c = icmp eq i8 %x, -1     ; samesign dropped, then further canonicalized
  ret i1 %c
}
```
The original `samesign ult` constrained `%x+1` and `%x` to the same sign
(i.e. `%x` not in {-1, INT_MAX}); the rewritten `eq -1` is a wider
specification.

## Why this is "only" information loss
`samesign ult (X+1), X` is poison whenever `(X+1)` and `X` have different
signs (overflow boundary). The rewrite to `eq X, -1` gives `true` only at
`X = -1`, which is the well-defined point where overflow happens. So the
rewrite preserves all defined cases and refines the poison case. Not a
miscompile.

## Fix sketch
Propagate `Pred.hasSameSign()` into every `new ICmpInst(...)` constructed
inside this function (5 sites). Since the original `samesign` constraint
ties the two operands of the icmp together, dropping it across the rewrite
loses an analysis-friendly fact.

## Notes
- Severity: missed-opt only.
- Same pattern (CmpPredicate received, raw Predicate emitted) appears
  throughout `InstCombineCompares.cpp` — this and w301 are representative.
