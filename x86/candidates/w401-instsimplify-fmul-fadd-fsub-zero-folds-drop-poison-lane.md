# instsimplify: identity-element folds (`X * 0`, `X + 0`, `X - 0`, `X & 0`, `X | -1`, `X ^ 0`, `X << 0`, `X + 0`, …) drop per-lane poison when the constant operand has a poison element

- Layer: middle-end (InstSimplify / LLVM IR)
- Pass: `instsimplify` (Analysis/InstructionSimplify.cpp)
- Architecture: target-independent; reproduces with x86 `-O2` and standalone `-passes=instsimplify`
- Severity: miscompile (poison lane silently replaced with a concrete value; `-O2` even attaches `noundef` to the resulting return value)
- LLVM HEAD tested: 23.0.0git, repo HEAD `0dd29960c`

## Summary

A whole family of folds in `InstructionSimplify.cpp` recognise an identity
element (FP zero, integer zero, all-ones, etc.) on the right-hand side via the
`m_AnyZeroFP()` / `m_PosZeroFP()` / `m_NegZeroFP()` / `m_Zero()` /
`m_AllOnes()` / `m_One()` family of matchers, and on a match return either
`Op0` (the unrelated operand) or a freshly synthesised splat constant
(`getNullValue`, `getAllOnesValue`, `ConstantFP::getZero`).

These matchers are built on `cstval_pred_ty<…, AllowPoison=true>`
(`llvm/include/llvm/IR/PatternMatch.h:317-358`), so a vector constant like
`<i32 0, i32 poison>` or `<float 0.0, float poison>` *matches* the predicate.
The fold then returns a whole vector with no `poison` lane — even though the
lane that was poison in the input must remain poison in the output (per
LangRef, every standard binop produces `poison` if any operand is `poison`).

This is a poison-refinement miscompile: a value that the language guarantees
is `poison` is replaced with a defined value. `-O2` will then mark the
enclosing function `noundef`, committing to the wrong behaviour.

The same structural issue affects at least these folds in
`llvm/lib/Analysis/InstructionSimplify.cpp`:

| Op       | Line(s)     | Fold                                | Failing input                |
|----------|-------------|-------------------------------------|------------------------------|
| `fmul`   | 6072-6075   | `X * 0 -> 0` (with nnan+nsz)        | `<float 0.0, float poison>`  |
| `fadd`   | 5938-5942   | `X + -0 -> X`                       | `<float -0.0, float poison>` |
| `fadd`   | 5945-5948   | `X + 0 -> X` (when X not -0 or nsz) | `<float 0.0, float poison>`  |
| `fsub`   | 5999-6004   | `X - 0 -> X`                        | `<float 0.0, float poison>`  |
| `fsub`   | 6006-6010   | `X - -0 -> X` (nsz / not -0)        | `<float -0.0, float poison>` |
| `and`    | 2088-2090   | `X & 0 -> 0`                        | `<i32 0, i32 poison>`        |
| `or`     | 2358-2359   | `X \| -1 -> -1`                     | `<i32 -1, i32 poison>`       |
| `or`     | 2362-2364   | `X \| 0 -> X`                       | `<i32 0, i32 poison>`        |
| `mul`    | 909-911     | `X * 0 -> 0`                        | `<i32 0, i32 poison>`        |
| `xor`    | (see file)  | `X ^ 0 -> X`                        | `<i32 0, i32 poison>`        |
| `sub`    | (see file)  | `X - 0 -> X`                        | `<i32 0, i32 poison>`        |
| `add`    | (see file)  | `X + 0 -> X`                        | `<i32 0, i32 poison>`        |
| `shl`    | (see file)  | `X << 0 -> X`                       | `<i32 0, i32 poison>`        |

Every one of these reproduces with the standalone test below; this report
treats them as a single bug because they share the same root cause (a
matcher that ignores poison lanes paired with a fold that does not).

## Root cause (source citations)

### The matchers

`llvm/include/llvm/IR/PatternMatch.h`

- `m_AnyZeroFP`, `m_PosZeroFP`, `m_NegZeroFP` — line 761-790, built from
  `cstfp_pred_ty<...>` which is `cstval_pred_ty<..., ConstantFP, AllowPoison=true>`
  (line 376-377).
- `m_Zero` — line 580-589, `is_zero` checks `C->isNullValue() ||
  cst_pred_ty<is_zero_int>().match(C)`. The latter has `AllowPoison=true`.
- `m_AllOnes` — `cst_pred_ty<is_all_ones>` with `AllowPoison=true`.
- The shared matcher class is `cstval_pred_ty::matchVector`, line 320-348:
  ```cpp
  if (AllowPoison && isa<PoisonValue>(Elt))
    continue;
  auto *CV = dyn_cast<ConstantVal>(Elt);
  if (!CV || !this->isValue(CV->getValue()))
    return false;
  ```
  Poison elements are silently skipped; the matcher reports success.

### The folds

`llvm/lib/Analysis/InstructionSimplify.cpp`

```cpp
// 2088-2090  (and)
if (match(Op1, m_Zero()))
  return Constant::getNullValue(Op0->getType());

// 2358-2359  (or with -1)
if (Q.isUndefValue(Op1) || match(Op1, m_AllOnes()))
  return Constant::getAllOnesValue(Op0->getType());

// 2362-2364  (or with 0 / X|X)
if (Op0 == Op1 || match(Op1, m_Zero()))
  return Op0;

// 909-911 (mul)
if (Q.isUndefValue(Op1) || match(Op1, m_Zero()))
  return Constant::getNullValue(Op0->getType());

// 6072-6075 (fmul)
if (match(Op1, m_AnyZeroFP())) {
  if (FMF.noNaNs() && FMF.noSignedZeros())
    return ConstantFP::getZero(Op0->getType());
  ...
}

// 5938-5942 (fadd -0) and 5945-5948 (fadd 0)
if (...) if (match(Op1, m_NegZeroFP())) return Op0;
if (...) if (match(Op1, m_PosZeroFP()) && ...) return Op0;

// 5999-6010 (fsub 0 / -0)
if (...) if (match(Op1, m_PosZeroFP())) return Op0;
if (...) if (match(Op1, m_NegZeroFP()) && ...) return Op0;
```

In every case the fold returns either:
- `Op0` — a whole vector that has no poison lane at the index that was poison
  in the matched constant; or
- `Constant::getNullValue(VecTy)` / `getAllOnesValue(VecTy)` /
  `ConstantFP::getZero(VecTy)` — a splat constant with a defined value in the
  formerly-poison lane.

Either way, the result has lost the poison that the matched operand carried.

The `simplifyFPOp` helper at line 5880-5917 *does* propagate poison, but only
when an *entire* operand is `PoisonValue` (line 5886-5887). It does not look
inside a `ConstantVector` of `<…, poison>`. The integer simplifiers do the
same `isa<PoisonValue>(Op1)` whole-vector check (e.g. line 2076-2078 in
`simplifyAndInst`).

### Why the matchers were intentionally lenient

`AllowPoison=true` is intentional for *commutative* predicates: e.g. for
`m_Zero()` used inside `m_Specific(constant)` style folds where the constant
is *used in proof of* the simplified result. The bug is that the folds
themselves treat the matcher's "all non-poison lanes meet the predicate" as
"every lane meets the predicate"; for `X op identity = X` to be valid
*per lane*, every lane of the identity operand really must be the identity
element — never poison.

## Reproducer 1 (fmul, the cleanest miscompile)

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
define noundef float @use_lane1(<2 x float> %x) local_unnamed_addr #0 {
  ret float 0.000000e+00
}
```

The function is now `noundef` and returns `0.0`. The source program's lane 1
is `%x[1] * poison = poison`, so the correct value is `poison`. The optimised
program returns a concrete `0.0` and even asserts via `noundef` that callers
may rely on the return being not-poison.

## Reproducer 2 (integer family, with `-O2` confirmation)

```llvm
target triple = "x86_64-unknown-linux-gnu"
define i32 @use_lane1(<2 x i32> %x) {
  %r = and <2 x i32> %x, <i32 0, i32 poison>
  %e = extractelement <2 x i32> %r, i64 1
  ret i32 %e
}
```

`opt -O2 -S`:

```llvm
define noundef i32 @use_lane1(<2 x i32> %x) local_unnamed_addr #0 {
  ret i32 0
}
```

## Reproducer 3 (full family, `-passes=instsimplify`)

```llvm
define <2 x i32> @add_zp(<2 x i32> %x) { %r = add <2 x i32> %x, <i32 0, i32 poison> ret <2 x i32> %r }
define <2 x i32> @sub_zp(<2 x i32> %x) { %r = sub <2 x i32> %x, <i32 0, i32 poison> ret <2 x i32> %r }
define <2 x i32> @xor_zp(<2 x i32> %x) { %r = xor <2 x i32> %x, <i32 0, i32 poison> ret <2 x i32> %r }
define <2 x i32> @shl_zp(<2 x i32> %x) { %r = shl <2 x i32> %x, <i32 0, i32 poison> ret <2 x i32> %r }
define <2 x i32> @and_z (<2 x i32> %x) { %r = and <2 x i32> %x, <i32 0,  i32 poison> ret <2 x i32> %r }
define <2 x i32> @mul_z (<2 x i32> %x) { %r = mul <2 x i32> %x, <i32 0,  i32 poison> ret <2 x i32> %r }
define <2 x i32> @or_n  (<2 x i32> %x) { %r = or  <2 x i32> %x, <i32 -1, i32 poison> ret <2 x i32> %r }
define <2 x float> @fadd_pzp(<2 x float> %x) { %r = fadd nnan nsz <2 x float> %x, <float 0.0,  float poison> ret <2 x float> %r }
define <2 x float> @fadd_nzp(<2 x float> %x) { %r = fadd <2 x float> %x, <float -0.0, float poison> ret <2 x float> %r }
define <2 x float> @fsub_pzp(<2 x float> %x) { %r = fsub <2 x float> %x, <float 0.0,  float poison> ret <2 x float> %r }
define <2 x float> @fmul_zp (<2 x float> %x) { %r = fmul nnan nsz <2 x float> %x, <float 0.0, float poison> ret <2 x float> %r }
```

`opt -passes=instsimplify -S`:

```llvm
define <2 x i32>   @add_zp(...) { ret <2 x i32> %x }
define <2 x i32>   @sub_zp(...) { ret <2 x i32> %x }
define <2 x i32>   @xor_zp(...) { ret <2 x i32> %x }
define <2 x i32>   @shl_zp(...) { ret <2 x i32> %x }
define <2 x i32>   @and_z (...) { ret <2 x i32> zeroinitializer }
define <2 x i32>   @mul_z (...) { ret <2 x i32> zeroinitializer }
define <2 x i32>   @or_n  (...) { ret <2 x i32> splat (i32 -1) }
define <2 x float> @fadd_pzp(...) { ret <2 x float> %x }
define <2 x float> @fadd_nzp(...) { ret <2 x float> %x }
define <2 x float> @fsub_pzp(...) { ret <2 x float> %x }
define <2 x float> @fmul_zp (...) { ret <2 x float> zeroinitializer }
```

Every result has a fully defined lane 1; the source semantics required lane 1
to be `poison`.

## Suggested fix

Guard each identity-element fold with
`!cast<Constant>(Op1)->containsPoisonElement()`. Cheap, conservative,
keeps the fast path for scalar / non-poison vector constants intact. For the
vector-with-poison case, either bail out (let downstream passes deal with the
explicit blend) or build an explicit per-lane result that preserves the
poison.

An analogous fix is needed in `simplifySelectInst` for the `m_Undef`,
`m_One`, and `m_Zero` paths — that is the subject of the separate w400
candidate. The two together suggest a general audit of every
`Constant::getNullValue(VecTy)` / `Constant::getAllOnesValue(VecTy)` return
site inside `InstructionSimplify.cpp` that is gated on a
`cstval_pred_ty<…, AllowPoison=true>` matcher of a vector operand.

## Hunt-area cross-reference

Triggered by the "simplifyFMul/simplifyFAdd with NaN+0 wrong" item. The NaN+0
case (the actual `nnan` gating) is correctly handled; the *poison*+0 case
exposes a much broader class of folds.
