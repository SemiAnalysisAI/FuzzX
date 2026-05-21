# InstCombine `((X << C) + Y) >>u C` rewrite drops inferable nuw/nsw flag on the new add

## Summary

The InstCombine fold

```
((X << C) + Y) >>u C  -->  (X + (Y >>u C)) & (-1 >>u C)
```

at `llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp:1535-1543` creates the
new `add` via `Builder.CreateAdd(NewLshr, X)` and the new `lshr` via
`Builder.CreateLShr(Y, Op1)` without propagating any of the original `add`'s
nuw/nsw or `lshr`'s `exact`. In particular, the new `add` is inferable to be
nuw whenever the original `add nuw` held: if `y + (x*2^C) < 2^N`, then
`x*2^C < 2^N`, so `x < 2^(N-C)`, so `x + (y>>C) < 2^(N-C) + 2^(N-C) <= 2^(N-C+1) <= 2^N`.

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp`:

```cpp
// 1532    // ((X << C) + Y) >>u C --> (X + (Y >>u C)) & (-1 >>u C)
// 1533    // TODO: Consolidate with the more general transform that starts from shl
// 1534    //       (the shifts are in the opposite order).
// 1535    if (match(Op0,
// 1536              m_OneUse(m_c_Add(m_OneUse(m_Shl(m_Value(X), m_Specific(Op1))),
// 1537                               m_Value(Y))))) {
// 1538      Value *NewLshr = Builder.CreateLShr(Y, Op1);
// 1539      Value *NewAdd = Builder.CreateAdd(NewLshr, X);
//   ...
// 1543      return BinaryOperator::CreateAnd(NewAdd, Mask);
```

Neither builder call receives any flag arguments, and there is no subsequent
`setHasNoUnsignedWrap`/`setHasNoSignedWrap` / `setIsExact` call. Compare with
nearby `factorizeMathWithShlOps` (`InstCombineAddSub.cpp:1497-1505`) which
correctly propagates nuw/nsw.

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i32 %x, i32 %y) {
  %a = shl i32 %x, 4
  %b = add nuw nsw i32 %a, %y
  %r = lshr i32 %b, 4
  ret i32 %r
}
```

Result:

```llvm
define i32 @f(i32 %x, i32 %y) {
  %1 = lshr i32 %y, 4          ; could be 'exact' when (a+y) low 4 bits are %y's
  %2 = add i32 %1, %x          ; lost: nuw (and nsw)
  %r = and i32 %2, 268435455
  ret i32 %r
}
```

The pre-fold `add nuw nsw` is dropped without inheritance. The post-fold add is
provably nuw whenever the pre-fold was, but the flag is not transferred.

## Impact

Missed optimization. Downstream passes (LSR, SCEV, codegen) can no longer use
the no-wrap fact about the residual add, which loses inferable knowledge such
as KnownBits / induction-variable simplification.

## Severity

Quality (missed optimization). Not a miscompile.
