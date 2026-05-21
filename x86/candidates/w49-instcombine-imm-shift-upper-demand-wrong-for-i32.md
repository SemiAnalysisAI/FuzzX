# X86 InstCombine: `simplifyX86immShift` over-conservatively demands an irrelevant element of the 128-bit shift-amount vector for 32-bit lane shifts

## File
`llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp`, lines 224-244.

## Code

```cpp
} else {
  // Ensure the first element has an in-range value and the rest of the
  // elements in the bottom 64 bits are zero.
  assert(AmtVT->isVectorTy() && AmtVT->getPrimitiveSizeInBits() == 128 &&
         cast<VectorType>(AmtVT)->getElementType() == SVT &&
         "Unexpected shift-by-scalar type");
  unsigned NumAmtElts = cast<FixedVectorType>(AmtVT)->getNumElements();
  APInt DemandedLower = APInt::getOneBitSet(NumAmtElts, 0);
  APInt DemandedUpper = APInt::getBitsSet(NumAmtElts, 1, NumAmtElts / 2);
  KnownBits KnownLowerBits = llvm::computeKnownBits(
      Amt, DemandedLower, II.getDataLayout());
  KnownBits KnownUpperBits = llvm::computeKnownBits(
      Amt, DemandedUpper, II.getDataLayout());
  if (KnownLowerBits.getMaxValue().ult(BitWidth) &&
      (DemandedUpper.isZero() || KnownUpperBits.isZero())) {
    ...
  }
}
```

## Issue

For shift intrinsics like `psll.d` (BitWidth=32, AmtVT=v4i32), hardware reads the *low 64 bits* of the 128-bit shift-amount vector as the shift count. In v4i32 layout that's elements 0 and 1: element 0 is the low 32 bits of the count, element 1 is the high 32 bits.

For the algebraic simplification to a generic `shl` of `splat(Amt[0])` to be valid:
- `Amt[0]` must be `< BitWidth` (in-range), AND
- `Amt[1]` must be zero (so the full 64-bit count equals `Amt[0]`).

`Amt[2]` and `Amt[3]` are *ignored* by the hardware — they are the upper 64 bits of the 128-bit vector.

The code computes `DemandedUpper = getBitsSet(NumAmtElts=4, 1, NumAmtElts/2=2)` → bit 1 only. So `DemandedUpper` correctly demands only element 1 for v4i32. Wait — `getBitsSet(width, lo, hi)` sets bits in `[lo, hi)`. So `(4, 1, 2)` sets only bit 1. That's element 1. **Correct for v4i32.**

For v8i16 (BitWidth=16, NumAmtElts=8): `getBitsSet(8, 1, 4)` sets bits 1,2,3 → elements 1,2,3 in the low 64 bits. **Correct.**

For v2i64 (BitWidth=64, NumAmtElts=2): `getBitsSet(2, 1, 1)` sets no bits → `DemandedUpper.isZero()` is true → skips the upper-zero check. **Correct.**

## Status

Re-analyzed and ruled out. The mask is exactly right.

## Confidence

Ruled out.
