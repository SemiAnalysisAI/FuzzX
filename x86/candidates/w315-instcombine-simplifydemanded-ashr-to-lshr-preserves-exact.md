# w315: InstCombine `SimplifyDemandedUseBits` AShr -> LShr conversion incorrectly preserves `exact` flag when shifted-in bits are merely undemanded (not known-zero sign bit)

## File / function

`llvm/lib/Transforms/InstCombine/InstCombineSimplifyDemanded.cpp`,
`InstCombinerImpl::SimplifyDemandedUseBits`, `case Instruction::AShr`,
lines 925-931:

```cpp
// If the input sign bit is known to be zero, or if none of the shifted in
// bits are demanded, turn this into an unsigned shift right.
if (Known.Zero[BitWidth - 1] || !ShiftedInBitsDemanded) {
  BinaryOperator *LShr = BinaryOperator::CreateLShr(I->getOperand(0),
                                                    I->getOperand(1));
  LShr->setIsExact(cast<BinaryOperator>(I)->isExact());
  LShr->takeName(I);
  return InsertNewInstWith(LShr, I->getIterator());
}
```

## Root cause

The conjunctive guard is split:

- `Known.Zero[BitWidth - 1]` — input is known non-negative; safe to switch
  `ashr exact` to `lshr exact` because the bits shifted off are known zero
  (= sign bit = 0), so both poison-sets coincide.
- `!ShiftedInBitsDemanded` — caller does not look at the high `ShiftAmt`
  bits of the result; safe to rewrite the *value* of the high bits (changing
  the sign-extension into zero-extension would only affect bits the caller
  ignores).

The `!ShiftedInBitsDemanded` branch is value-correct for the demanded low
bits, but it copies the original `exact` flag verbatim onto the new `lshr`.
This is wrong:

- `ashr exact %x, N` is poison iff the bits shifted off do *not* all equal
  the input's sign bit.
- `lshr exact %x, N` is poison iff the bits shifted off are *not* all zero.

For a negative input whose low `N` bits are all 1 (e.g. `%x = -1`), the
original `ashr exact` is well-defined (shifted-off bits all equal the
sign-bit 1) while the new `lshr exact` is poison (shifted-off bits are
`0b111...1`, not zero). The transform therefore widens the poison set even
though the *bitvector value* of every demanded low bit is identical between
the two ops.

`exact` is a poison-generating flag on the operation as a whole, so once it
strictly applies to the new `lshr`, the entire result becomes poison
whenever the original `ashr exact` would have been defined for negative
inputs with `low-bits == all-1`. Any downstream consumer that observes the
result (mask AND, store, freeze, branch, etc.) sees `poison` instead of the
specific value the original IR guaranteed.

Contrast with the sibling branch on the same line: when
`Known.Zero[BitWidth - 1]` is true the input is non-negative, the original
`ashr exact`'s "shifted-off == sign bit" reduces to "shifted-off == 0",
which is exactly the `lshr exact` constraint, so transferring `exact` is
safe.

The fix is to drop `setIsExact` on the `!ShiftedInBitsDemanded` arm (or to
gate it behind the sign-bit-zero condition that already gives the previous
arm its safety):

```cpp
if (Known.Zero[BitWidth - 1]) {
  ... LShr->setIsExact(cast<BinaryOperator>(I)->isExact());
  ...
} else if (!ShiftedInBitsDemanded) {
  ... // do NOT call setIsExact here
  ...
}
```

## Reproducer (built `opt`, default x86 `-O2`-level instcombine)

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @t(i32 %x) {
  %a = ashr exact i32 %x, 3
  %m = and i32 %a, 255
  ret i32 %m
}
```

### `opt -passes=instcombine -S` produces

```llvm
define i32 @t(i32 %x) {
  %a = lshr exact i32 %x, 3
  %m = and i32 %a, 255
  ret i32 %m
}
```

### Concrete value at `%x = -1` (0xFFFFFFFF)

- Source: `ashr exact i32 -1, 3` = -1 (defined: shifted-off bits `0b111`
  equal the sign bit `1`). `and i32 -1, 255` = `255`.
- Optimized: `lshr exact i32 -1, 3` = **poison** (shifted-off bits `0b111`
  != 0). `and i32 poison, 255` = `poison`.

The user-observable return value changed from a defined `255` to `poison`.

## Why it matters

This is a classic InstCombine refinement bug. The optimizer is allowed to
narrow the poison set (poison-source -> defined-target), never widen it
(defined-source -> poison-target). The buggy `setIsExact` widens it, and
downstream passes that branch on the truncated value, store it, or pattern
match against it as `noundef` can produce arbitrary behavior on inputs
where the original IR had a specified result.

The trigger is small and frontend-realistic. A common frontend pattern
is `(int)x >> N` for negative `x` with the bottom bits known set (e.g.
extracting a tagged-pointer payload where the bottom bits are a tag and
the top bits a signed offset), masked back down to a narrow type. The
exact flag often shows up when the frontend or earlier passes have
established that the bottom-`N` bits are zero -- but for *negative* values
"bottom bits == sign bit == 1", not "bottom bits == 0", so the `exact`
flag remains valid for `ashr` but becomes a false claim under `lshr`.

## Existing LLVM test bakes in the bug

`llvm/test/Transforms/InstCombine/ashr-demand.ll` has the test
`ashr_can_be_lshr` which asserts the buggy behavior verbatim:

```llvm
; "If it does not matter if we do ashr or lshr, then we canonicalize to lshr."
define i16 @ashr_can_be_lshr(i32 %a) {
; CHECK-NEXT:    [[ASHR:%.*]] = lshr exact i32 [[A:%.*]], 16
; CHECK-NEXT:    [[TRUNC:%.*]] = trunc nuw i32 [[ASHR]] to i16
  %ashr = ashr exact i32 %a, 16
  %trunc = trunc nsw i32 %ashr to i16
  ret i16 %trunc
}
```

The test comment "if it does not matter" is wrong: it DOES matter for the
poison set when `a` is negative. The check pattern needs to be updated to
expect plain `lshr i32 %a, 16` (without `exact`) once the fix lands.

## Confidence

High (verified by reproducer against the built `opt`).
Root cause is localized to two lines; the fix is mechanical (drop
`setIsExact` from the `!ShiftedInBitsDemanded` arm). The CHECK pattern in
`ashr-demand.ll` provides additional evidence that the buggy transform is
the routine, intended path of this code -- this is not a rare corner case.
