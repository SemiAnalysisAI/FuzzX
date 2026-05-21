# InstCombine `sqrt(X)*sqrt(Y) -> sqrt(X*Y)` requires nnan but not ninf — folds finite product to +inf

File: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp`, function
`InstCombinerImpl::foldFMulReassoc`, lines 864-872.

## Pattern

```cpp
// sqrt(X) * sqrt(Y) -> sqrt(X * Y)
// nnan disallows the possibility of returning a number if both operands are
// negative (in that case, we should return NaN).
if (I.hasNoNaNs() && match(Op0, m_OneUse(m_Sqrt(m_Value(X)))) &&
    match(Op1, m_OneUse(m_Sqrt(m_Value(Y))))) {
  Value *XY = Builder.CreateFMulFMF(X, Y, &I);
  Value *Sqrt = Builder.CreateUnaryIntrinsic(Intrinsic::sqrt, XY, &I);
  return replaceInstUsesWith(I, Sqrt);
}
```

The comment justifies requiring `nnan` (so `sqrt(-X)*sqrt(-Y)` does not
silently produce a real number where the original would have produced NaN).
But the transform also assumes that `X * Y` is finite when `sqrt(X) * sqrt(Y)`
is — and that is not true in IEEE-754 for finite large `X` and `Y`. The fold
silently introduces `+inf` results that did not exist in the source.

## Repro

```llvm
; opt -passes=instcombine -S
define float @bug(float %x, float %y) {
  %a = call float @llvm.sqrt.f32(float %x)
  %b = call float @llvm.sqrt.f32(float %y)
  %r = fmul nnan reassoc float %a, %b
  ret float %r
}
declare float @llvm.sqrt.f32(float)
```

Output:

```llvm
define float @bug(float %x, float %y) {
  %1 = fmul reassoc nnan float %x, %y
  %r = call reassoc nnan float @llvm.sqrt.f32(float %1)
  ret float %r
}
```

## Why it's wrong

The instruction is annotated only with `nnan reassoc`. Neither flag licenses
the optimizer to turn a finite result into an infinite one. With `X = Y = 1e30f`:

- Source:  `sqrt(1e30) * sqrt(1e30) = ~1e15 * ~1e15 = ~1e30` (representable
  in f32 — the f32 max is ≈3.4e38).
- Folded:  `sqrt(1e30 * 1e30) = sqrt(1e60_f32) = sqrt(+inf) = +inf`.

`reassoc` permits reassociation but not the introduction of an overflow that
the source did not have. Per the LLVM langref, `ninf` is the flag that
licenses "treat the result as not being infinity" / drop infinity behaviors;
absent it, an optimizer must not promote a finite value to inf. (Other
sqrt-related transforms in this same function — e.g. line 891 `(X / sqrt(Y))²
-> X*X / Y`, line 880 `1/sqrt(X) * X -> X/sqrt(X)` — correctly require `nsz`,
showing the existing pattern of stacking the necessary flags.)

The fix is to additionally gate on `I.hasNoInfs()`, mirroring how `nnan`
guards the negative-input case. A weaker fix would be to bail out when
`X * Y` would overflow but `sqrt(X) * sqrt(Y)` would not, but checking that
generally requires range info that is not always available; requiring `ninf`
on the original fmul matches the flag the user is asked to provide to
license the algebraic identity that doesn't preserve the finite/infinite
boundary.

The same line of reasoning would also catch underflow on the inner product
for very small `X` and `Y`: `sqrt(tiny) * sqrt(tiny)` may be representable
while `tiny * tiny` underflows to 0, and `sqrt(0) = 0` — different from
the source's nonzero result. `ninf` does not cover that case either, so a
proper fix should also include something stronger (e.g. `arcp`/`nsz` for
the zero subtlety, or simply restrict the transform to constants for which
the new multiply is known finite and nonzero).
