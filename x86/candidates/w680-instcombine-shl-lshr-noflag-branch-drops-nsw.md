# InstCombine `(X << C1) >>u C` (C1 > C, no nuw) drops inferable nsw on the new shl

## Summary

The `visitLShr` fold

```
(X << C1) >>u C  -->  (X << (C1 - C)) & (-1 >>u C)      ; if C1 > C, Op0 has one-use, no nuw
```

at `llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp:1518-1523` builds the
new `shl` via `Builder.CreateShl(X, ShiftDiff)` without propagating any IR
flags from the original `shl`. In particular, if the original `shl` was `nsw`,
the new `shl X, (C1 - C)` is *also* `nsw`, but the flag is dropped.

The sibling `nuw` branch immediately above (lines 1511-1517) propagates
nuw and even infers nsw via `setHasNoSignedWrap(ShAmtC > 0)`. The
`hasOneUse` no-flag branch has no analogous propagation.

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp`:

```cpp
// 1508    } else if (C1->ugt(ShAmtC)) {
// 1509      unsigned ShlAmtC = C1->getZExtValue();
// 1510      Constant *ShiftDiff = ConstantInt::get(Ty, ShlAmtC - ShAmtC);
// 1511      if (cast<BinaryOperator>(Op0)->hasNoUnsignedWrap()) {
// 1512        // (X <<nuw C1) >>u C --> X <<nuw/nsw (C1 - C)
// 1513        auto *NewShl = BinaryOperator::CreateShl(X, ShiftDiff);
// 1514        NewShl->setHasNoUnsignedWrap(true);
// 1515        NewShl->setHasNoSignedWrap(ShAmtC > 0);
// 1516        return NewShl;
// 1517      }
// 1518      if (Op0->hasOneUse()) {
// 1519        // (X << C1) >>u C  --> X << (C1 - C) & (-1 >> C)
// 1520        Value *NewShl = Builder.CreateShl(X, ShiftDiff);   ; <-- no flags
// 1521        APInt Mask(APInt::getLowBitsSet(BitWidth, BitWidth - ShAmtC));
// 1522        return BinaryOperator::CreateAnd(NewShl, ConstantInt::get(Ty, Mask));
// 1523      }
// 1524    } else {
```

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i32 %x) {
  %s1 = shl nsw i32 %x, 8
  %s2 = lshr i32 %s1, 4
  ret i32 %s2
}
```

After `opt -passes=instcombine -S`:

```llvm
define i32 @f(i32 %x) {
  %1 = shl i32 %x, 4         ; nsw dropped (and could be inferred)
  %s2 = and i32 %1, 268435440
  ret i32 %s2
}
```

The pre-fold `shl nsw i32 %x, 8` constrains `%x` to have at least 8+1 = 9
sign bits (countMinSignBits >= 9). The post-fold `shl i32 %x, 4` would be
`nsw` iff `%x` has at least 4+1 = 5 sign bits. 9 >= 5, so `nsw` holds and
should be set on the new shl.

(More generally, this branch applies for any `C1 > C >= 1`. Original
`shl nsw C1` ensures `countMinSignBits(X) > C1`. New `shl (C1 - C)` is
`nsw` iff `countMinSignBits(X) > C1 - C`, which follows from `C >= 1`.)

## Impact

Missed optimization. The lost `nsw` blocks subsequent passes (later
InstCombine cycles, SCEV, LSR, codegen) from inferring KnownBits/sign
information about the shifted value. The `nuw`-flag branch one if-statement
above (1511-1516) correctly preserves both flags; the no-nuw branch is
inconsistent.

## Severity

Quality (missed optimization). Not a miscompile.
