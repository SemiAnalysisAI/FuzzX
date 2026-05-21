# instsimplify: simplifyFMul / simplifyFAdd / simplifyFSub "X * 0", "X + 0", "X - 0" folds drop per-lane poison when the constant operand has a poison element

- Layer: middle-end (InstSimplify / LLVM IR)
- Pass: `instsimplify` (Analysis/InstructionSimplify.cpp)
- Architecture: target-independent; reproduces with x86 `-O2` and standalone `-passes=instsimplify`
- Severity: miscompile (poison lane silently replaced with a concrete value; even `noundef` attribute is later attached to the result by `-O2`)
- LLVM HEAD tested: 23.0.0git, repo HEAD `0dd29960c`

## Summary

Several scalar-friendly folds in `InstructionSimplify.cpp` use the
`m_AnyZeroFP()` / `m_PosZeroFP()` / `m_NegZeroFP()` matchers to fold
binary FP ops to the other operand (or to zero) when one operand is a
floating-point zero. Those matchers are built from
`cstval_pred_ty<…, ConstantFP, /*AllowPoison=*/true>`
(`llvm/include/llvm/IR/PatternMatch.h:374-377` and the matcher class at
`317-358`), which means a vector constant like `<float 0.0, float poison>`
matches the predicate — the poison lane is treated as "ignore". The folds
themselves then return either the unrelated operand (`Op0` / `X`) or a freshly
synthesised splat zero, in both cases producing a vector that has a defined
value where the source vector required `poison`.

LangRef rule: any FP arithmetic with a `poison` operand produces `poison`
(it is one of the canonical poison-propagating ops). Specifically,
`fmul X, poison`, `fadd X, poison`, and `fsub X, poison` all produce `poison`
in the corresponding lane. The fold is therefore a poison-refinement
miscompile.

The bug applies identically to:
- `simplifyFMAFMul` at line 6072-6088 (the `X * 0` fold);
- `simplifyFAddInst` at line 5938-5948 (the `X + -0` and `X + 0` folds);
- `simplifyFSubInst` at line 5999-6010 (the `X - 0` and `X - -0` folds).

All three rely on `m_AnyZeroFP` / `m_PosZeroFP` / `m_NegZeroFP`. Because the
ConstantFP-predicate matchers allow poison lanes to "match", a partial-poison
vector trips the shortcut and the simplifier returns `Op0` (i.e. the
unmodified other operand), silently dropping the poison.

## Root cause (source citations)

`llvm/lib/Analysis/InstructionSimplify.cpp`:

- `simplifyFMAFMul`, line 6072-6075:

  ```cpp
  if (match(Op1, m_AnyZeroFP())) {
    // X * 0.0 --> 0.0 (with nnan and nsz)
    if (FMF.noNaNs() && FMF.noSignedZeros())
      return ConstantFP::getZero(Op0->getType());
  ```

  `ConstantFP::getZero(VecTy)` produces a *splat* zero — every lane is the
  literal +0.0 — even if `Op1` was `<0.0, poison>`. Lane 1 was required to be
  `poison` (because `X[1] * poison = poison`); it is now `0.0`.

- `simplifyFAddInst`, line 5938-5942 (`fadd X, -0`) and line 5945-5948
  (`fadd X, 0`):

  ```cpp
  if (canIgnoreSNaN(...) && (... || FMF.noSignedZeros()))
    if (match(Op1, m_NegZeroFP()))
      return Op0;
  ...
  if (canIgnoreSNaN(...))
    if (match(Op1, m_PosZeroFP()) &&
        (FMF.noSignedZeros() || cannotBeNegativeZero(Op0, Q)))
      return Op0;
  ```

  `Op0` is the full vector; the poison lanes of `Op1` are discarded.

- `simplifyFSubInst`, line 5999-6010 (`fsub X, +0` and `fsub X, -0`):

  ```cpp
  if (...)
    if (match(Op1, m_PosZeroFP()))
      return Op0;
  if (...)
    if (match(Op1, m_NegZeroFP()) &&
        (FMF.noSignedZeros() || cannotBeNegativeZero(Op0, Q)))
      return Op0;
  ```

The `simplifyFPOp` helper at line 5880-5917 *does* propagate poison, but only
for an *entire* `PoisonValue` operand (line 5886-5887:
`if (any_of(Ops, IsaPred<PoisonValue>)) return PoisonValue::get(...)`).
A `ConstantVector` of `<0.0, poison>` is *not* a `PoisonValue`, so the per-lane
poison slips past that guard, then the matchers' `AllowPoison=true` semantics
cause the fold to fire.

`m_AnyZeroFP` is defined at `PatternMatch.h:761`:

```cpp
inline cstfp_pred_ty<is_any_zero_fp> m_AnyZeroFP() {
  return cstfp_pred_ty<is_any_zero_fp>();
}
```

with `cstfp_pred_ty` being `cstval_pred_ty<..., ConstantFP, /*AllowPoison=*/true>`.

## Reproducer 1: `fmul nnan nsz X, <0.0, poison>` (line 6072 path)

`/tmp/llvmtest/fmul_use_o2.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
define float @use_lane1(<2 x float> %x) {
  %r = fmul nnan nsz <2 x float> %x, <float 0.0, float poison>
  %e = extractelement <2 x float> %r, i64 1
  ret float %e
}
```

`opt -O2 -S` (LLVM HEAD `0dd29960c`):

```llvm
; Function Attrs: mustprogress nofree norecurse nosync nounwind willreturn memory(none)
define noundef float @use_lane1(<2 x float> %x) local_unnamed_addr #0 {
  ret float 0.000000e+00
}
```

Notes:
- The result is `0.000000e+00`, not `poison`.
- The function is now marked `noundef`, meaning the compiler has *committed*
  that a poison return would be UB. So if the source program is allowed to
  rely on the return being poison (and a downstream caller does a
  poison-triggering use), the optimised program will silently behave
  differently.

`opt -passes=instsimplify -S` on the same input shows the exact transform:

```llvm
define float @use_lane1(<2 x float> %x) {
  ret float 0.000000e+00
}
```

## Reproducer 2: `fadd X, <0.0, poison>` and `fadd X, <-0.0, poison>`

```llvm
define <2 x float> @fadd_pzp(<2 x float> %x) {
  %r = fadd nnan nsz <2 x float> %x, <float 0.0, float poison>
  ret <2 x float> %r
}
define <2 x float> @fadd_negzp(<2 x float> %x) {
  %r = fadd <2 x float> %x, <float -0.0, float poison>
  ret <2 x float> %r
}
```

`opt -passes=instsimplify -S`:

```llvm
define <2 x float> @fadd_pzp(<2 x float> %x)  { ret <2 x float> %x }
define <2 x float> @fadd_negzp(<2 x float> %x) { ret <2 x float> %x }
```

Lane 1 of the source was `%x[1] + poison = poison`; lane 1 of the optimised
result is `%x[1]`, a defined value.

## Reproducer 3: `fsub X, <0.0, poison>` and `fsub nsz X, <-0.0, poison>`

```llvm
define <2 x float> @fsub_pzp(<2 x float> %x) {
  %r = fsub <2 x float> %x, <float 0.0, float poison>
  ret <2 x float> %r
}
define <2 x float> @fsub_negz_poison(<2 x float> %x) {
  %r = fsub nsz <2 x float> %x, <float -0.0, float poison>
  ret <2 x float> %r
}
```

`opt -passes=instsimplify -S`:

```llvm
define <2 x float> @fsub_pzp(<2 x float> %x)         { ret <2 x float> %x }
define <2 x float> @fsub_negz_poison(<2 x float> %x) { ret <2 x float> %x }
```

## Why this is the *same* root cause across three folds, not three separate bugs

All three folds funnel through the `m_AnyZeroFP` / `m_PosZeroFP` / `m_NegZeroFP`
family of matchers, which share the `cstval_pred_ty<…, ConstantFP, AllowPoison=true>`
implementation. The `AllowPoison=true` semantics were intentional for *predicates*
on integer constants (and for FP NaN/Inf matching), but for FP zero folds the
right side of the implication "if every non-poison lane is a zero, return Op0"
is wrong: even one poison lane on the RHS forces the corresponding lane of the
result to be poison, and the result cannot equal `Op0`.

The bug is therefore structurally identical to my w400 candidate
(`simplifySelectInst` accepting poison lanes via `m_One`/`m_Zero`/`m_Undef`):
in both, an `AllowPoison=true` matcher is paired with a fold that returns a
*whole* vector rather than per-lane.

## Suggested fix

Either:
(a) gate the three FP-zero folds on
    `cast<Constant>(Op1)->containsPoisonElement() == false` (cheap, conservative
    win for the common scalar case);

(b) introduce poison-preserving variants — e.g. `m_PosZeroFP_NoPoison()` — and
    use them in `simplifyFAddInst`, `simplifyFSubInst`, `simplifyFMAFMul`;

(c) in `simplifyFPOp`, generalise the all-`PoisonValue` check to "if any lane
    of any operand is poison, then for each such lane the corresponding result
    lane is poison" and return a *blended* vector. This is more work but
    captures more cases.

Option (a) is the minimum to make these miscompiles go away and is the
pattern used for `simplifySelect` (`Constant::containsPoisonElement()`).

## Hunt-area cross-reference

This was triggered by re-reading the "simplifyFMul/simplifyFAdd with NaN+0
wrong" item in the brief. The NaN+0 angle didn't reveal anything (those folds
are gated correctly on `canIgnoreSNaN` and `FMF.noSignedZeros()`/
`cannotBeNegativeZero`); however the *poison*+0 angle, hit while exhaustively
testing `<…, poison>` vector inputs, did.
