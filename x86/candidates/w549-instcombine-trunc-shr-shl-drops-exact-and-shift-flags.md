# InstCombine `(trunc (X >> C1)) << C` rewrite drops `exact`/nuw/nsw flags on the new shift

## Summary

The fold

```
(trunc (X >> C1)) << C  -->  (trunc (X >> (C1 - C))) & (-1 << C)    (if C1 > C)
(trunc (X >> C1)) << C  -->  (trunc (X << (C - C1))) & (-1 << C)    (if C  > C1)
```

builds the new wide shift via `Builder.CreateBinOp(ShiftOpc, X, ShiftDiffC,
"sh.diff")` without copying flags from the original inner `lshr/ashr` or
outer `shl`.

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp`:

```cpp
// 1248  BinaryOperator *Shr;
// 1249  if (match(Op0, m_OneUse(m_Trunc(m_OneUse(m_BinOp(Shr))))) &&
// 1250      match(Shr, m_Shr(m_Value(X), m_APInt(C1)))) {
// 1251    // The larger shift direction survives through the transform.
// 1252    unsigned ShrAmtC = C1->getZExtValue();
// 1253    unsigned ShDiff = ShrAmtC > ShAmtC ? ShrAmtC - ShAmtC : ShAmtC - ShrAmtC;
// 1254    Constant *ShiftDiffC = ConstantInt::get(X->getType(), ShDiff);
// 1255    auto ShiftOpc = ShrAmtC > ShAmtC ? Shr->getOpcode() : Instruction::Shl;
// 1256
// 1257    // If C1 > C:
// 1258    // (trunc (X >> C1)) << C --> (trunc (X >> (C1 - C))) && (-1 << C)
// 1259    // If C > C1:
// 1260    // (trunc (X >> C1)) << C --> (trunc (X << (C - C1))) && (-1 << C)
// 1261    Value *NewShift = Builder.CreateBinOp(ShiftOpc, X, ShiftDiffC, "sh.diff");
// 1262    Value *Trunc = Builder.CreateTrunc(NewShift, Ty, "tr.sh.diff");
// 1263    APInt Mask(APInt::getHighBitsSet(BitWidth, BitWidth - ShAmtC));
// 1264    return BinaryOperator::CreateAnd(Trunc, ConstantInt::get(Ty, Mask));
// 1265  }
```

The inner shift's `exact` is provably preserved when `ShrAmtC > ShAmtC`
(shifting by fewer bits keeps the divisibility relation), but the new shift
gets no flags. Likewise the new `trunc` does not inherit `nuw`/`nsw`.

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i64 %x) {
  %s1 = lshr exact i64 %x, 16
  %t = trunc i64 %s1 to i32
  %s2 = shl nuw nsw i32 %t, 8
  ret i32 %s2
}
```

Result:

```llvm
define i32 @f(i64 %x) {
  %sh.diff = lshr i64 %x, 8       ; lost: exact (provably preserved since shift smaller)
  %tr.sh.diff = trunc i64 %sh.diff to i32
  %s2 = and i32 %tr.sh.diff, -256
  ret i32 %s2
}
```

## Impact

Missed optimization. `exact` is the bit that downstream passes (LSR, SCEV,
`isPowerOfTwo`) rely on for division-style transforms.

## Severity

Quality. Not a miscompile.
