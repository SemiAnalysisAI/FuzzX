# InstCombine `FoldShiftByConstant` pull-through of binop-with-constant-RHS drops `disjoint` on `or`

## Summary

The `FoldShiftByConstant` rewrite

```
shift (binop X, C0), C1  -->  binop (shift X, C1), (shift C0, C1)
```

at `llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp:957-973` creates
the new `binop` with `BinaryOperator::Create(Op0BO->getOpcode(), NewShift,
NewRHS)` at line 970 and never copies any IR flags from the source binop.
For `or disjoint`, the flag is *provably preserved* by the rewrite: if
`X AND C0 == 0`, then `(X shl C1) AND (C0 shl C1) == 0` (bit positions
shift uniformly), so the new `or` is still disjoint.

The same applies to `lshr`/`ashr` of the constant (bitwise operations
commute with shifts position-wise).

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp:957-973`:

```cpp
// 957    if (auto *Op0BO = dyn_cast<BinaryOperator>(Op0)) {
// 958      // If the operand is a bitwise operator with a constant RHS, and the
// 959      // shift is the only use, we can pull it out of the shift.
// 960      const APInt *Op0C;
// 961      if (match(Op0BO->getOperand(1), m_APInt(Op0C))) {
// 962        if (canShiftBinOpWithConstantRHS(I, Op0BO)) {
// 963          Value *NewRHS =
// 964              Builder.CreateBinOp(I.getOpcode(), Op0BO->getOperand(1), C1);
// 965
// 966          Value *NewShift =
// 967              Builder.CreateBinOp(I.getOpcode(), Op0BO->getOperand(0), C1);
// 968          NewShift->takeName(Op0BO);
// 969
// 970          return BinaryOperator::Create(Op0BO->getOpcode(), NewShift, NewRHS);
// 971        }
// 972      }
// 973    }
```

Compare with the analogous fold at `InstCombineShifts.cpp:1466-1469`
(handling `((X << nuw Z) binop nuw Y) >>u Z`) which *does* propagate
`disjoint`:

```cpp
// 1466      } else if (auto *Disjoint = dyn_cast<PossiblyDisjointInst>(Op0)) {
// 1467        cast<PossiblyDisjointInst>(NewBinOp)->setIsDisjoint(
// 1468            Disjoint->isDisjoint());
// 1469      }
```

The asymmetry makes the omission a clean missed-opt.

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i32 %x) {
  %a = or disjoint i32 %x, 7
  %r = shl i32 %a, 4
  ret i32 %r
}
```

After `opt -passes=instcombine -S`:

```llvm
define i32 @f(i32 %x) {
  %a = shl i32 %x, 4
  %r = or i32 %a, 112       ; lost: disjoint (provably preserved)
  ret i32 %r
}
```

### Why `disjoint` is provably valid on the new `or`

Original `or disjoint X, 7` asserts `X AND 7 == 0`, i.e., `X[0..3) = 0`.
After `shl 4`, the new shifted X = `X << 4` has bits at positions [4, 36)
populated by X's bits [0, 32). The constant `7 << 4 = 112` populates bits
[4, 7). Their AND is `X[0..3) shifted to positions [4..7)` ANDed with bits
[4..7) of `112`. Both regions correspond to `X[0..3) AND 1` which is `0`
(from original disjoint). So `(X << 4) AND 112 == 0`, i.e., new `or` is
disjoint.

For `lshr`/`ashr` the bit positions move by the same amount on both
operands so the disjointness is similarly preserved.

## Impact

Missed optimization. Codegen can lower disjoint `or` as `add nuw` (on x86,
this enables `lea` for indexed-address-arithmetic). The drop forces a
plain `or` lowering.

## Severity

Quality (missed optimization). Not a miscompile.
