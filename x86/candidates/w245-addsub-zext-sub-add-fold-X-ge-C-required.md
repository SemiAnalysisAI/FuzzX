# w245: (add (zext (add X, -C)), C) -> (zext X) when X u>= C

## File / Region
- `llvm/lib/Transforms/InstCombine/InstCombineAddSub.cpp`,
  `InstCombinerImpl::foldAddWithConstant`, lines ~1007-1023.

## Code
```cpp
// Fold (add (zext (add X, -C)), C) -> (zext X) if X u>= C.
// Truncate C to the narrow type to avoid mismatched width comparisons.
{
  const APInt *InnerC;
  if (match(Op0, m_ZExt(m_Add(m_Value(X), m_APIntAllowPoison(InnerC))))) {
    unsigned NarrowBW = InnerC->getBitWidth();
    if (C->isIntN(NarrowBW)) {
      APInt NarrowC = C->trunc(NarrowBW);
      const SimplifyQuery Q = SQ.getWithInstruction(&Add);
      if (*InnerC == -NarrowC &&
          (NarrowC.isOne()
               ? llvm::isKnownNonZero(X, Q)
               : computeKnownBits(X, &Add).getMinValue().uge(NarrowC)))
        return new ZExtInst(X, Ty);
    }
  }
}
```

## Observation
This fold collapses `zext(X - C) + C` into `zext(X)` when X is known
to be unsigned >= C. The condition `(X u>= C)` is required to prevent
underflow in the inner `add X, -C`.

## Analysis (Alive2-style)
If X u>= C: `X - C` is well-defined (no underflow in unsigned sense),
`zext(X - C) = X - C` (as a wider int), `(X - C) + C = X = zext(X)`. **Match.**

If X u< C: `X - C` underflows. Treated as unsigned in i8: e.g., X=3, C=5,
X-C = 254. zext = 254. +5 = 259. But the fold would give zext(X) = 3.
**Different.**

The KnownBits check `getMinValue().uge(NarrowC)` correctly gates the
fold to only fire when X >= C is provable.

The special case `NarrowC.isOne() ? isKnownNonZero(X, Q)` handles C=1
specially because `isKnownNonZero` is a stronger query than KnownBits
min-value being 1 in some cases.

## Reproducer (positive — fold fires correctly)
Source: `/tmp/w240/t45_zext_sub_add.ll`

```llvm
define i32 @zext_sub_add_known(i8 %x) {
  %m = or i8 %x, 5    ; ensures M >= 5 (bits 0 and 2 set)
  %s = sub i8 %m, 5
  %z = zext i8 %s to i32
  %r = add i32 %z, 5
  ret i32 %r
}
```
folds to:
```llvm
define i32 @zext_sub_add_known(i8 %x) {
  %1 = or i8 %x, 5
  %r = zext i8 %1 to i32
  ret i32 %r
}
```

The KnownBits analysis correctly determines `(X | 5) u>= 5` and fires
the fold.

## Verdict
**NOT a miscompile.** The KnownBits gating is correct. Documented for
completeness.
