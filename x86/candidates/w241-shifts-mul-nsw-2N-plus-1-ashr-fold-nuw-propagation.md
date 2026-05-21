# w241: ashr(mul nsw (X, 2^N+1), N) -> add(X, ashr(X, N)) — nuw propagation review

## File / Region
- `llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp`,
  `InstCombinerImpl::visitAShr`, lines ~1902-1915.

## Code
```cpp
const APInt *MulC;
if (match(Op0, m_OneUse(m_NSWMul(m_Value(X), m_APInt(MulC)))) &&
    (BitWidth > 2 && (*MulC - 1).isPowerOf2() &&
     MulC->logBase2() == ShAmt &&
     (ShAmt < BitWidth - 1))) /* Minus 1 for the sign bit */ {

  // ashr (mul nsw (X, 2^N + 1)), N -> add nsw (X, ashr(X, N))
  auto *NewAdd = BinaryOperator::CreateNSWAdd(
      X,
      Builder.CreateAShr(X, ConstantInt::get(Ty, ShAmt), "", I.isExact()));
  NewAdd->setHasNoUnsignedWrap(
      cast<OverflowingBinaryOperator>(Op0)->hasNoUnsignedWrap());
  return NewAdd;
}
```

## Observation
The fold transforms `(X *nsw (2^N+1)) ashr N` into `X +nsw (X ashr N)`,
propagating NUW from the mul to the add.

## Analysis (Alive2-style)
For `X * (2^N + 1) = X*2^N + X = (X << N) + X`:
- ashr by N of `(X << N) + X` equals `X + (X ashr N)` for any X (the
  identity `(a << N + b) ashr N = a + (b ashr N)` when `(a << N)` doesn't
  overflow nsw — which the `mul nsw` precondition gives us).

NUW propagation:
- If `mul nuw` (with positive C = 2^N+1), then X is non-negative
  (unsigned interpretation requires X * C < 2^BW, and if X were "negative"
  i.e., MSB set, then X*C overflows quickly).
- For X >= 0, `ashr X N = lshr X N`, both non-negative.
- New add: X + (X lshr N). For X >= 0 and X * (2^N+1) < 2^BW (nuw on mul):
  X + X/2^N <= X*(1 + 1/2^N) <= X * (2^N+1)/2^N. Since mul nuw gives
  X*(2^N+1) < 2^BW, we have X + X/2^N < 2^BW / 2^N <= 2^BW. So no unsigned
  wrap on the add.

The fold is **correct**.

Test `mul nsw nuw i8 X, 5; ashr i8 _, 2`:
```llvm
define i8 @mul_ashr_5_nuw(i8 %x) {
  %m = mul nsw nuw i8 %x, 5
  %r = ashr i8 %m, 2
  ret i8 %r
}
```
folds to:
```llvm
%1 = lshr i8 %x, 2
%r = add nuw nsw i8 %x, %1
```
(Note: `ashr` is canonicalized to `lshr` because mul-nuw implies X >= 0.)

## Verdict
**NOT a miscompile** — fold reviewed and is correct. Documented for completeness.
