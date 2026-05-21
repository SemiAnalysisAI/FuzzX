# InstCombine `exp(X)*exp(Y) -> exp(X+Y)` (and `exp2`) require only reassoc — folds NaN to a finite number on overflow/underflow

File: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp`, function
`InstCombinerImpl::foldFMulReassoc`, lines 933-948.

## Pattern

```cpp
if (I.isOnlyUserOfAnyOperand()) {
  // ...
  // exp(X) * exp(Y) -> exp(X + Y)
  if (match(Op0, m_Intrinsic<Intrinsic::exp>(m_Value(X))) &&
      match(Op1, m_Intrinsic<Intrinsic::exp>(m_Value(Y)))) {
    Value *XY = Builder.CreateFAddFMF(X, Y, &I);
    Value *Exp = Builder.CreateUnaryIntrinsic(Intrinsic::exp, XY, &I);
    return replaceInstUsesWith(I, Exp);
  }

  // exp2(X) * exp2(Y) -> exp2(X + Y)
  if (match(Op0, m_Intrinsic<Intrinsic::exp2>(m_Value(X))) &&
      match(Op1, m_Intrinsic<Intrinsic::exp2>(m_Value(Y)))) {
    Value *XY = Builder.CreateFAddFMF(X, Y, &I);
    Value *Exp2 = Builder.CreateUnaryIntrinsic(Intrinsic::exp2, XY, &I);
    return replaceInstUsesWith(I, Exp2);
  }
}
```

Inside `foldFMulReassoc` so `reassoc` is guaranteed. No other FMF required.

The identity `e^X · e^Y = e^(X+Y)` is exact over the reals but not in IEEE-754
f32 (and similar in f16/f64 at appropriate ranges) when one factor overflows
and the other underflows. The source produces `+inf · 0 = NaN`; the fold
produces `exp(0) = 1.0`.

## Repro

```llvm
; opt -passes=instcombine -S
define float @bug(float %x, float %y) {
  %ex = call float @llvm.exp.f32(float %x)
  %ey = call float @llvm.exp.f32(float %y)
  %r = fmul reassoc float %ex, %ey
  ret float %r
}
declare float @llvm.exp.f32(float)
```

Output:

```llvm
define float @bug(float %x, float %y) {
  %1 = fadd reassoc float %x, %y
  %r = call reassoc float @llvm.exp.f32(float %1)
  ret float %r
}
```

Witness (constant inputs to force evaluation):

```llvm
define float @check() {
  %ex = call float @llvm.exp.f32(float 200.0)   ; -> +inf in f32 (exp 200 ~ 7.2e86 vs max ~3.4e38)
  %ey = call float @llvm.exp.f32(float -200.0)  ; -> 0    in f32 (exp -200 ~ 1.4e-87 vs min denormal ~1.4e-45)
  %r = fmul reassoc float %ex, %ey
  ret float %r
}
; folds to: ret float +qnan
```

So with `%x = 200, %y = -200`, the source returns NaN but the folded
function returns `exp(0) = 1.0`. A program flag is set wrongly: only
`reassoc` is required, but a result that was NaN under the source becomes
a finite 1.0 — a transformation that requires `nnan`.

## Why this is wrong

`reassoc` does not authorize introducing or removing infinities or NaNs;
only `ninf` covers infinity-related changes and only `nnan` covers
NaN-related changes. The fold removes a NaN result that arose from `inf *
0`, which can only be justified by `nnan` (the user asserting the source
never reaches the inf*0 case).

The `exp2` variant has the same defect at smaller magnitudes (`exp2(200)`
overflows in f32; `exp2(-200)` underflows). Both should be guarded by
`I.hasNoNaNs() && I.hasNoInfs()`, mirroring the pattern of the
`(X / sqrt(Y))^2` fold at line 891 (which requires `nnan` and `nsz`) and
the `1/sqrt(X) * X` fold at line 880 (which requires `nsz`).

## Suggested fix

```cpp
if (I.hasNoNaNs() && I.hasNoInfs() && I.isOnlyUserOfAnyOperand()) { /* ... */ }
```

or move the `exp`/`exp2` clauses out of the `isOnlyUserOfAnyOperand` block
into a block gated by `nnan ninf`.
