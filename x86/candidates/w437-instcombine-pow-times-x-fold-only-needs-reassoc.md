# InstCombine `pow(X, Y) * X -> pow(X, Y+1)` requires only reassoc — folds NaN to 1.0

File: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp`, function
`InstCombinerImpl::foldFMulReassoc`, lines 905-913.

## Pattern

```cpp
// pow(X, Y) * X --> pow(X, Y+1)
// X * pow(X, Y) --> pow(X, Y+1)
if (match(&I, m_c_FMul(m_OneUse(m_Intrinsic<Intrinsic::pow>(m_Value(X),
                                                            m_Value(Y))),
                       m_Deferred(X)))) {
  Value *Y1 = Builder.CreateFAddFMF(Y, ConstantFP::get(I.getType(), 1.0), &I);
  Value *Pow = Builder.CreateBinaryIntrinsic(Intrinsic::pow, X, Y1, &I);
  return replaceInstUsesWith(I, Pow);
}
```

This fires inside `foldFMulReassoc`, so `reassoc` is the only guaranteed
flag. The mathematical identity `X^Y * X = X^(Y+1)` is exact over the reals
but not over IEEE-754 with `pow` semantics: at `X = +0`, `Y = -1` we have
`pow(0, -1) = +inf` (C99 §F.10.4.4 / libm convention), so

- Source:  `pow(0, -1) * 0 = +inf * 0 = NaN`.
- Folded:  `pow(0, -1 + 1) = pow(0, 0) = 1.0` (the standard convention).

The transform replaces a NaN-producing computation with a finite 1.0 under
nothing but `reassoc`, which only authorizes reassociation, not the removal
of NaN results.

## Repro

```llvm
; opt -passes=instcombine -S
define float @bug(float %x, float %y) {
  %p = call reassoc float @llvm.pow.f32(float %x, float %y)
  %r = fmul reassoc float %p, %x
  ret float %r
}
declare float @llvm.pow.f32(float, float)
```

Output:

```llvm
define float @bug(float %x, float %y) {
  %1 = fadd reassoc float %y, 1.000000e+00
  %r = call reassoc float @llvm.pow.f32(float %x, float %1)
  ret float %r
}
```

At runtime with `%x = 0.0, %y = -1.0`:
- Source returns NaN.
- Folded function returns `pow(0, 0) = 1.0`.

## Why this is unsound

`pow(0, -1)` is a finite-input call that returns `+inf` per LLVM langref
("matches the standard `pow` function"). The fmul of inf with the original
`%x = 0` is then required to yield NaN under IEEE-754. The folded program
calls `pow(0, 0) = 1.0`. The flags `reassoc` (and even adding `arcp` or
`contract`) do not license replacing a NaN result with a finite result.
`nnan` is required for that, and additionally either `ninf` or a guard
against `Y = -1` (so the multiply-by-X never sees `inf * 0`) is necessary.

The companion transform two lines below (`foldPowiReassoc` at lines 634-720,
particularly the `powi(X, Y) * X -> powi(X, Y+1)` clause at lines 650-660)
applies `willNotOverflowSignedAdd(Y, One, I)` because the exponent is an
integer; for `pow` with a floating-point exponent there is no comparable
safety check.

## Fix

Add `I.hasNoNaNs()` to the gate, or strengthen to `I.hasNoNaNs() &&
I.hasNoInfs()` to additionally cover the symmetric case where `pow(0, +1)
* X` would compute `0 * 0 = 0` differently from `pow(0, 2)` — though that
particular case is fine, the general rule should be that any transform
that can erase a NaN result must require `nnan`.
