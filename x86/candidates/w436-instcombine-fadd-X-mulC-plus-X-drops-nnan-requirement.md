# InstCombine `(X * MulC) + X -> X * (MulC + 1.0)` turns NaN result into +inf without nnan

File: `llvm/lib/Transforms/InstCombine/InstCombineAddSub.cpp`, function
`InstCombinerImpl::visitFAdd`, lines 2162-2168 (inside `if
(I.hasAllowReassoc() && I.hasNoSignedZeros())` block at line 2135).

## Pattern

```cpp
// (X * MulC) + X --> X * (MulC + 1.0)
Constant *MulC;
if (match(&I, m_c_FAdd(m_FMul(m_Value(X), m_ImmConstant(MulC)),
                       m_Deferred(X)))) {
  if (Constant *NewMulC = ConstantFoldBinaryOpOperands(
          Instruction::FAdd, MulC, ConstantFP::get(I.getType(), 1.0), DL))
    return BinaryOperator::CreateFMulFMF(X, NewMulC, &I);
}
```

The transform requires `reassoc + nsz` (from the enclosing if at line 2135).
It does NOT require `nnan` or `ninf`. The algebraic identity `X*C + X =
X*(C+1)` is exact for real numbers but not for IEEE-754 when `X` is `+/-inf`
and `MulC` is zero: the source produces NaN via `inf*0 + inf`, the folded
form preserves the infinity.

## Repro

```llvm
; opt -passes=instcombine -S
define float @bug(float %x) {
  %m = fmul reassoc nsz float %x, 0.0
  %r = fadd reassoc nsz float %m, %x
  ret float %r
}
```

Output:

```llvm
define float @bug(float %x) {
  ret float %x
}
```

Witness: passing `%x = +inf` (e.g. via `fdiv 1.0, 0.0` from another function
or as a function argument) shows the divergence. With constant inputs the
constant folder demonstrates the source's result:

```llvm
define float @check() {
  %m = fmul reassoc nsz float 0x7FF0000000000000, 0.0  ; inf * 0 = NaN
  %r = fadd reassoc nsz float %m, 0x7FF0000000000000   ; NaN + inf = NaN
  ret float %r
}
; folds to: ret float +qnan
```

So with `%x = +inf` the source yields `+qnan` but the optimized form yields
`+inf`. The two diverge for the same input under flags (`reassoc nsz`) that
do not authorize the conversion.

## Why `reassoc + nsz` is not enough

- `nsz` only licenses ignoring the sign of zeros, not changing whether the
  result is a number, infinity, or NaN.
- `reassoc` per LangRef licenses reassociation that "may dramatically change
  results in floating-point", but the intent is reassociation of operands /
  evaluation order. Turning a NaN result into a non-NaN result requires
  `nnan` (so the optimizer may assume the NaN-producing case does not occur,
  i.e. is poison).

Other transforms in this very function correctly carry the `nnan` flag for
the same algebraic rearrangement when an inf×0 / inf-inf case is exposed
— see `simplifyFAddInst` line 5953-5972 (`-X + X -> 0` only under `nnan`)
and the comment at line 5960 ("We don't have to explicitly exclude
infinities (ninf): INF + -INF == NaN").

## Fix

Add `I.hasNoNaNs()` to the gate of the transform, or more precisely require
`nnan` whenever `MulC + 1.0` would be `1.0` (i.e. `MulC == 0`) — that is the
only case where the source can hide a NaN created by `inf*0` that the
folded form would expose as a non-NaN.
