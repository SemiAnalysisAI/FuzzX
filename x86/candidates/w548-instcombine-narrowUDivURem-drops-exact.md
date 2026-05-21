# InstCombine `narrowUDivURem` drops `exact` flag when sinking through zext

## Summary

The narrowing fold

```
udiv (zext X), (zext Y)  -->  zext (udiv X, Y)
urem (zext X), (zext Y)  -->  zext (urem X, Y)
```

builds the narrow operation via `IC.Builder.CreateBinOp(Opcode, X, Y)`
without copying `exact` from the original wide div, so `udiv exact` becomes
plain `udiv`.

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp`:

```cpp
// 1656  if (match(N, m_ZExt(m_Value(X))) && match(D, m_ZExt(m_Value(Y))) &&
// 1657      X->getType() == Y->getType() && (N->hasOneUse() || D->hasOneUse())) {
// 1658    // udiv (zext X), (zext Y) --> zext (udiv X, Y)
// 1659    // urem (zext X), (zext Y) --> zext (urem X, Y)
// 1660    Value *NarrowOp = IC.Builder.CreateBinOp(Opcode, X, Y);
// 1661    return new ZExtInst(NarrowOp, Ty);
// 1662  }
```

The same bug applies to the constant-RHS variants at lines 1675 and 1686.

`udiv exact` is preserved by narrowing because:
`zext(udiv X Y) == udiv(zext X, zext Y)` and the divisibility relation between
operands is unchanged.

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i16 %x, i16 %y) {
  %zx = zext i16 %x to i32
  %zy = zext i16 %y to i32
  %r = udiv exact i32 %zx, %zy
  ret i32 %r
}
```

Result:

```llvm
define i32 @f(i16 %x, i16 %y) {
  %1 = udiv i16 %x, %y    ; lost: exact
  %r = zext i16 %1 to i32
  ret i32 %r
}
```

## Impact

Missed optimization. `exact` lets downstream KnownBits, demanded-bits, and
loop-strength-reduction passes reason about the residual bits being zero.

## Severity

Quality. Not a miscompile.
