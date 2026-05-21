# InstCombine `shl/lshr/ashr (select C, (binop X, C1), X), C2` drops nuw/nsw/exact/disjoint on every new instruction

## Summary

The fold in `InstCombinerImpl::FoldShiftByConstant`

```
shl (select C, (add X, C1), X), C2  -->  Y = shl X, C2;
                                          select C, (add Y, C1 << C2), Y
```

(and the mirror form starting from the false arm being the binop) builds three
new instructions via raw `Builder.CreateBinOp(...)` without copying or
intersecting any flags from the original outer shift or inner binop.

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp`:

```cpp
// 984    BinaryOperator *TBO;
// 985    Value *FalseVal;
// 986    if (match(Op0, m_Select(m_Value(Cond), m_OneUse(m_BinOp(TBO)),
// 987                            m_Value(FalseVal)))) {
//   ...
// 992        Value *NewRHS =
// 993            Builder.CreateBinOp(I.getOpcode(), TBO->getOperand(1), C1);
// 994
// 995        Value *NewShift = Builder.CreateBinOp(I.getOpcode(), FalseVal, C1);
// 996        Value *NewOp = Builder.CreateBinOp(TBO->getOpcode(), NewShift, NewRHS);
// 997        return SelectInst::Create(Cond, NewOp, NewShift);
// 998      }
// 999    }

// the mirror branch starting at line 1001 has the same problem
```

All three `CreateBinOp` callers (`NewRHS`, `NewShift`, `NewOp`) leave the
default no-flag instruction; the inner `add nuw nsw` and outer `shl nuw nsw`
flags are discarded.

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i1 %c, i32 %x) {
  %a = add nuw nsw i32 %x, 5
  %s = select i1 %c, i32 %a, i32 %x
  %r = shl nuw nsw i32 %s, 2
  ret i32 %r
}
```

Result:

```llvm
define i32 @f(i1 %c, i32 %x) {
  %1 = shl i32 %x, 2          ; lost: nuw and nsw (transfer-from outer shift)
  %2 = add i32 %1, 20         ; lost: nuw and nsw (transfer-from inner add)
  %r = select i1 %c, i32 %2, i32 %1
  ret i32 %r
}
```

The new `shl X, C2` is provably nuw / nsw whenever the outer `shl Op0, C2`
was — `Op0` could take either select arm, so `X` (the false arm) and `X+5`
(the true arm via the inner add) must both not overflow the shift.

Similarly the new `add Y, 20` (= `add (X<<2), 20`) inherits nuw / nsw from the
original `add X, 5` (with constant operand scaled by `<<2`).

## Impact

Missed optimization. Both shift-by-constant and add-by-constant lose
provably-correct no-wrap flags. This blocks subsequent KnownBits / range
inference. For `or disjoint`, the same code path silently drops the disjoint
flag (no separate test, but the fold also handles `or` via `m_BinOp(TBO)`).

## Severity

Quality. Not a miscompile.
