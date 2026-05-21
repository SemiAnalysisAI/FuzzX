# w629: narrowBinOp drops nsw/nuw and exact flags when narrowing trunc(binop)

## Source
- File: `llvm/lib/Transforms/InstCombine/InstCombineCasts.cpp`
- Function: `InstCombinerImpl::narrowBinOp`
- Lines: 856-887 (And/Or/Xor/Add/Sub/Mul cases),
        889-913 (LShr/AShr preserve exact via `OldShift->isExact()`)

## Code

```cpp
case Instruction::And:
case Instruction::Or:
case Instruction::Xor:
case Instruction::Add:
case Instruction::Sub:
case Instruction::Mul: {
  Constant *C;
  if (match(BinOp0, m_Constant(C))) {
    // trunc (binop C, X) --> binop (trunc C', X)
    Constant *NarrowC = ConstantExpr::getTrunc(C, DestTy);
    Value *TruncX = Builder.CreateTrunc(BinOp1, DestTy);
    return BinaryOperator::Create(BinOp->getOpcode(), NarrowC, TruncX);
    //     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //     no flags carried — Add/Sub/Mul nsw/nuw and Shl nsw/nuw are dropped
  }
  ...
}
```

## Repro: `/tmp/icc625/trunc-add.ll`

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i8 @t(i8 %a) {
  %sa = sext i8 %a to i32
  %s  = shl nsw nuw i32 %sa, 1
  %t  = trunc i32 %s to i8
  ret i8 %t
}
```

`opt -passes=instcombine -S`:

```llvm
define i8 @t(i8 %a) {
  %s = shl i8 %a, 1       ; nsw/nuw lost
  ret i8 %s
}
```

For Add/Sub/Mul narrowing: nsw/nuw should be dropped (overflow semantics
change when narrowing) — this is correct. But for `Shl`, narrowing by a
shift amount that's strictly less than the narrow bit width can preserve
nuw if the original `Shl nuw` shifted into bits that get truncated away —
in those cases the narrow shift is non-overflowing too.

## Analysis

This is a missed optimization, not a soundness bug:
- For Add/Sub/Mul, dropping nsw/nuw on narrowing is necessary.
- For And/Or/Xor, there are no nsw/nuw flags.
- For Shl with constant amount C < DestWidth, the narrowed `Shl X, C` may
  inherit `nuw` from the wide version (the truncated-away upper bits of
  the wide result correspond to the same overflow check on the narrow
  side, modulo X's value, which `MaskedValueIsZero` could prove).
- For Shl nsw, similar reasoning.

Also see `EvaluateInDifferentTypeImpl` (lines 55-75) which has the same
issue for the deeper canEvaluate path.

## Severity

Soundness-preserving missed optimization. Combines with w628 for the
broader pattern of `EvaluateInDifferentType` + `narrowBinOp` dropping
poison-generating flags wherever they could legitimately survive.

## Hunt brief alignment

Cited as a candidate for the "sext/zext chained through select with
mismatched flags" theme generalized to "trunc/sext/zext chained through
narrowed binops with mismatched flags".
