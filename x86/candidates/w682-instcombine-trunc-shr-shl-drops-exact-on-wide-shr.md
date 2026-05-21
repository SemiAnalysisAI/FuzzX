# InstCombine `(trunc (X >>? C1)) << C` drops `exact` flag from the original shr

## Summary

The `visitShl` "look through trunc" fold

```
(trunc (X >>? C1)) << C
  -->  (trunc (X >>? (C1 - C))) & (-1 << C)   ; if C1 > C, opcode = original shr
  -->  (trunc (X << (C - C1)))  & (-1 << C)   ; if C >= C1, opcode = Shl
```

at `llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp:1247-1265` creates
the new wide shift via `Builder.CreateBinOp(ShiftOpc, X, ShiftDiffC, ...)`
without any flag, even when the original `Shr` was marked `exact`.

For the `C1 > C` branch (wide right shift survives), an `exact` on the
original `X >>? C1` implies low `C1` bits of `X` are zero, which strictly
exceeds the `C1 - C` low bits required for the new shr to be `exact`. So
`exact` is straightforwardly preserved but is dropped.

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp:1247-1265`:

```cpp
// 1247    // Similar to above, but look through an intermediate trunc instruction.
// 1248    BinaryOperator *Shr;
// 1249    if (match(Op0, m_OneUse(m_Trunc(m_OneUse(m_BinOp(Shr))))) &&
// 1250        match(Shr, m_Shr(m_Value(X), m_APInt(C1)))) {
// 1251      // The larger shift direction survives through the transform.
// 1252      unsigned ShrAmtC = C1->getZExtValue();
// 1253      unsigned ShDiff = ShrAmtC > ShAmtC ? ShrAmtC - ShAmtC : ShAmtC - ShrAmtC;
// 1254      Constant *ShiftDiffC = ConstantInt::get(X->getType(), ShDiff);
// 1255      auto ShiftOpc = ShrAmtC > ShAmtC ? Shr->getOpcode() : Instruction::Shl;
//   ...
// 1261      Value *NewShift = Builder.CreateBinOp(ShiftOpc, X, ShiftDiffC, "sh.diff");
// 1262      Value *Trunc = Builder.CreateTrunc(NewShift, Ty, "tr.sh.diff");
// 1263      APInt Mask(APInt::getHighBitsSet(BitWidth, BitWidth - ShAmtC));
// 1264      return BinaryOperator::CreateAnd(Trunc, ConstantInt::get(Ty, Mask));
// 1265    }
```

No `setIsExact` on the new shr; no `setHasNoUnsignedWrap`/`setHasNoSignedWrap`
on the new shl.

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i64 %x) {
  %s1 = lshr exact i64 %x, 60
  %t = trunc i64 %s1 to i32
  %s2 = shl i32 %t, 5
  ret i32 %s2
}
```

After `opt -passes=instcombine -S`:

```llvm
define i32 @f(i64 %x) {
  %sh.diff = lshr i64 %x, 55              ; lost: 'exact' (provably preserved)
  %tr.sh.diff = trunc nuw nsw i64 %sh.diff to i32
  %s2 = and i32 %tr.sh.diff, 480
  ret i32 %s2
}
```

### Why `exact` is provably valid on the new shr

Original `lshr exact i64 %x, 60` implies `%x`'s low 60 bits are zero
(otherwise the shift would lose information and become poison).

The new shift is `lshr i64 %x, 55`. For this to be `exact`, the low 55 bits
of `%x` must be zero. Since 60 > 55, this strictly follows from the original
exact, so `exact` is preserved and should be set on the new lshr.

The same argument applies to the `ashr exact` variant (since the rewrite
keeps the original shr's opcode).

## Impact

Missed optimization. The lost `exact` blocks later analyses (KnownBits,
SCEV, codegen `bextr`/`sarx`-style lowering on x86) from exploiting the
trailing-zero structure of the source value.

## Severity

Quality (missed optimization). Not a miscompile.
