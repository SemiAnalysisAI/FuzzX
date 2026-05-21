# 223 — `ExpandIRInsts::expandFPToI` saturating path lets `+Inf`/`-Inf` produce a non-saturated value for `iN+` (N > BitWidth - IsSigned + ExpBias)

Component: `llvm/lib/CodeGen/ExpandIRInsts.cpp` lines ~692-714

The saturating arm threshold-tests `BiasedExp >= ExpBias + BitWidth - IsSigned`. For `f32` (max BiasedExp = 255) targeting `i256` (BitWidth = 256), threshold = `127 + 256 - 0 = 383` >> 255. So `+Inf` (BiasedExp = 255) falls into the ExpLargeBB normal path and produces approximately `2^128` instead of `UINT256_MAX`.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll -o -` for `llvm.fptoui.sat.i256.f32(float +Inf)`:

Produces a store of `(0, 1, 0, 0)` representing `2^128` instead of all-ones (UINT256_MAX). Expected behavior of saturating fptoui.sat per LangRef: +Inf saturates to UINT256_MAX (all bits set).

## Severity

Default x86 -O2 codegen miscompile for source-level saturating float→int with i129+ targets.

## Fix

Re-check the threshold: should be `BitWidth - IsSigned >= max_biased_exp_of_FltTy - ExpBias` (in `f32` units, anything ≥ 128 bits saturates if input is Inf).
