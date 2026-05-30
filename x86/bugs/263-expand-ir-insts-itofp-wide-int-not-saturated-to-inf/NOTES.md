# 263 — `ExpandIRInsts::expandIToFP` lets a wide-int `sitofp`/`uitofp` overflow to garbage instead of `Inf`

Component: `llvm/lib/CodeGen/ExpandIRInsts.cpp`, `expandIToFP` (the int→FP expansion used for integers wider than the legal set, e.g. `i129`/`i256`).

`s`/`uitofp` of an integer whose magnitude exceeds the largest finite value of
the destination FP type must produce `+Inf`/`-Inf`. The software expansion
assembles the result as `(unbiasedExp << MantissaWidth) + bias`, ORed with the
sign and mantissa — with **no check that the exponent fits the FP exponent
field**. Once `unbiasedExp` reaches `1 << (ExponentWidth - 1)` the value wraps
into the sign/garbage range instead of saturating to `Inf`.

This is the int→FP mirror of #223 (the `fpto*i.sat` direction). It can't happen
for the usual `i32 -> float` etc. (the integer can't exceed the FP range), only
for the over-wide integers handled by this pass.

## Reproducer

`opt -S -mtriple=x86_64-- --expand-ir-insts repro.ll`: the expansion of
`uitofp i129 -> float` contains no `0x7FF0000000000000` saturation `select`
(the fix, PR #200291, adds one).

Self-contained constant: `uitofp i256 2^200 to float` must be `+Inf`
(`0x7F800000`). The unfixed expansion computes exponent field `201 << 23 + bias
= 0xA4000000` (≈ `-2.66e-17`) — a finite, wrong-signed garbage value.

## Severity

Default x86 `-O2` codegen miscompile for source-level `sitofp`/`uitofp` from
`i129`+ integers (over-wide ints lowered by ExpandIRInsts). Wrong result class
(finite/NaN instead of `Inf`).

## Fix

PR [#200291](https://github.com/llvm/llvm-project/pull/200291) (merged) — after
building the FP value, `select` a correctly-signed infinity when the unbiased
exponent reaches `1 << (ExponentWidth - 1)`; skip the check when even
`BitWidth - 1` can't reach that threshold. Fixes llvm#189054.
