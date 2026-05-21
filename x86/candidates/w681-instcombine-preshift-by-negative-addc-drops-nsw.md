# InstCombine pre-shift-of-constant-by-(X+NegC) drops nsw on the new shl

## Summary

The `commonShiftTransforms` fold for variable-amount-with-negative-offset

```
C << (X + NegAddC)  -->  (C >>u PosOffset) << X      ; when (C >> PosOff) << PosOff == C
```

at `llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp:472-502` creates the
new `shl` and explicitly propagates only `nuw`:

```cpp
// 496      if (I.getOpcode() == Instruction::Shl) {
// 497        NewShiftOp->setHasNoUnsignedWrap(I.hasNoUnsignedWrap());
// 498      } else {
// 499        NewShiftOp->setIsExact();
// 500      }
```

There is no corresponding `setHasNoSignedWrap(I.hasNoSignedWrap())`. When the
original shift had `nsw` (and not `nuw`), the suitability guard at line 482
still permits the rewrite:

```cpp
// 482    case Instruction::Shl:
// 483      return (I.hasNoSignedWrap() || I.hasNoUnsignedWrap()) &&
// 484             AC->eq(AC->lshr(PosOffset).shl(PosOffset));
```

so we apply the fold but lose the `nsw` flag on the new `shl`.

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp:491-501`:

```cpp
// 491      if (isSuitableForPreShift()) {
// 492        Constant *NewC = ConstantInt::get(Ty, I.getOpcode() == Instruction::Shl
// 493                                                  ? AC->lshr(PosOffset)
// 494                                                  : AC->shl(PosOffset));
// 495        BinaryOperator *NewShiftOp =
// 496            BinaryOperator::Create(I.getOpcode(), NewC, A);
// 497        if (I.getOpcode() == Instruction::Shl) {
// 498          NewShiftOp->setHasNoUnsignedWrap(I.hasNoUnsignedWrap());
// 499        } else {
// 500          NewShiftOp->setIsExact();
// 501        }
// 502        return NewShiftOp;
// 503      }
```

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i32 %x) {
  %a = add i32 %x, -4
  %r = shl nsw i32 16, %a
  ret i32 %r
}
```

After `opt -passes=instcombine -S`:

```llvm
define i32 @f(i32 %x) {
  %r = shl nuw i32 1, %x          ; nsw should also be set (inferred), but isn't
  ret i32 %r
}
```

(`nuw` is added later by `setShiftFlags` via KnownBits on the constant 1; it
is not coming from the original IR's flags. The original `nsw` is silently
dropped.)

### Why `nsw` is provably valid on the result

Original `shl nsw 16, (X-4)` requires the signed product `16 * 2^(X-4)` to
fit in a signed i32 = `[-2^31, 2^31)`. Since 16 > 0, this constrains
`X - 4 < 27`, i.e. `X < 31`.

Post-fold `shl 1, X` is `nsw` iff `1 * 2^X` fits in signed i32, i.e. `X < 31`
(equivalently `X <= 30`).

The two constraints are identical, so `nsw` should propagate verbatim from
the original shift. (The same equivalence holds for arbitrary `AC` in this
fold: the new shl by `X` operates on the *pre-shifted* constant
`AC >>u PosOffset`, with the relationship between `X` and the shift bound
exactly mirroring the original.)

## Impact

Missed optimization. The dropped `nsw` blocks downstream nsw-dependent
folds (range analysis, SCEV iv-step inference, codegen sign-bit
exploitation). The sibling AShr/LShr branch correctly propagates `exact`,
making the asymmetry stand out.

## Severity

Quality (missed optimization). Not a miscompile.
