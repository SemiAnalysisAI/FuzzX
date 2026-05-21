# InstCombine `foldShiftOfShiftedBinOp` drops `disjoint` flag on the rewritten or

## Summary

The transform

```
shift (binop (shift X, C0), Y), C1
  -->  binop (shift X, C0+C1), (shift Y, C1)
```

at `llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp:353-408`
(`foldShiftOfShiftedBinOp`) constructs the new `binop` with
`BinaryOperator::Create(BinInst->getOpcode(), Op1, Op2)` at line 407 and
never copies any IR flags from the original `BinInst`. For `or` with
`disjoint`, the flag is *provably preserved* by the rewrite but is dropped.

The intuition: `disjoint` on `(X' | Y)` (where `X' = X shl C0`) asserts
`X' AND Y == 0`. Shifting both sides of the AND by `C1` (the outer shift)
yields `(X' shl C1) AND (Y shl C1) == 0`, which is exactly the disjoint
condition on the new `or`. The same reasoning works for lshr/ashr (bit-
positional reasoning is identical).

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp`:

```cpp
// 401    // shift (binop (shift X, C0), Y), C1 -> binop (shift X, C0+C1), (shift Y, C1)
// 402    Constant *ShiftSumC = ConstantExpr::getAdd(C0, C1);
// 403    Value *NewShift1 = Builder.CreateBinOp(ShiftOpcode, X, ShiftSumC);
// 404    Value *NewShift2 = Builder.CreateBinOp(ShiftOpcode, Y, C1);
// 405    Value *Op1 = FirstShiftIsOp1 ? NewShift2 : NewShift1;
// 406    Value *Op2 = FirstShiftIsOp1 ? NewShift1 : NewShift2;
// 407    return BinaryOperator::Create(BinInst->getOpcode(), Op1, Op2);
```

No `cast<PossiblyDisjointInst>(...)->setIsDisjoint(...)` or similar; the
new BinaryOperator inherits no flags.

(For Add/Sub the drop of nuw/nsw is in general necessary because of the
distribution-vs-mod-2^N interaction. The disjoint-on-or case is unique in
being unambiguously preservable, which makes the omission a clean
missed-opt.)

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i32 %x, i32 %y) {
  %a = shl i32 %x, 2
  %b = or disjoint i32 %a, %y
  %r = shl i32 %b, 3
  ret i32 %r
}
```

After `opt -passes=instcombine -S`:

```llvm
define i32 @f(i32 %x, i32 %y) {
  %1 = shl i32 %x, 5
  %2 = shl i32 %y, 3
  %r = or i32 %1, %2          ; lost: disjoint (provably preserved)
  ret i32 %r
}
```

## Impact

Missed optimization. `disjoint` enables downstream passes (`add`-vs-`or`
canonicalization, BoolToOptimalAddRecur, codegen lea-formation on x86) to
treat the `or` as equivalent to `add nuw`. Dropping it on every
shift-of-shifted-binop limits how far that propagates.

## Severity

Quality (missed optimization). Not a miscompile.
