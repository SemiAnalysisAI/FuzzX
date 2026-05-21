# w243: shift-by-(srem X, pow2) replaced with shift-by-(X & (pow2-1))

## File / Region
- `llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp`,
  `commonShiftTransforms`, lines ~539-547.

## Code
```cpp
// Canonicalize: shift X, (srem A, power_of_2) -> shift X, (A & (pow2-1))
if (Op1->hasOneUse() && match(Op1, m_SRem(m_Value(A), m_Constant(C))) &&
    match(C, m_Power2())) {
  // FIXME: Should this get moved into SimplifyDemandedBits by saying we don't
  // demand the sign bit (and many others) here??
  Constant *Mask = ConstantExpr::getSub(C, ConstantInt::get(Ty, 1));
  Value *Rem = Builder.CreateAnd(A, Mask, Op1->getName());
  return replaceOperand(I, 1, Rem);
}
```

## Observation
The fold replaces `shift X, (srem Y, 2^N)` with `shift X, (Y & (2^N-1))`.
For negative Y, `srem` produces a negative remainder (sign-of-dividend
rule), which makes the shift poison (shift amount >= bitwidth in two's
complement). The AND folds the negative case to a positive value, which
makes the shift defined.

## Analysis (Alive2-style)
- For Y >= 0: `Y srem 2^N == Y & (2^N - 1)`. Same shift amount, same result.
- For Y < 0: `Y srem 2^N` is negative or zero (sign-of-dividend). Shift by
  negative value = poison. `Y & (2^N - 1)` is the low N bits of Y's
  two's-complement representation, always in [0, 2^N-1]. Shift defined.

The fold is a **refinement of poison** to a defined value for negative Y,
which is sound in LLVM.

## Reproducer
Source: `/tmp/w240/t44_srem_shamt.ll`

```llvm
define i32 @shl_srem(i32 %x, i32 %y) {
  %r = srem i32 %y, 8
  %s = shl i32 %x, %r
  ret i32 %s
}
```

`opt -passes=instcombine -S` output:
```llvm
define i32 @shl_srem(i32 %x, i32 %y) {
  %r1 = and i32 %y, 7
  %s = shl i32 %x, %r1
  ret i32 %s
}
```

For Y = -10, X = 1:
- Original: `R = -10 srem 8 = -2`, `shl 1, -2` is poison.
- Folded: `R = -10 & 7 = 6`, `shl 1, 6 = 64`.

## Verdict
**NOT a miscompile.** Refinement of poison (negative Y case) to a defined
value is sound. Documented because the behavior change between original
and folded versions for negative Y could appear surprising.
