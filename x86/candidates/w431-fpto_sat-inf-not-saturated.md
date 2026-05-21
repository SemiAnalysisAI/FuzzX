# w431: ExpandIRInsts `expandFPToI` saturating path lets `+Inf` / `-Inf` escape saturation when the destination width exceeds the FP exponent range

## Where

`llvm/lib/CodeGen/ExpandIRInsts.cpp:692-714` — the `IsSaturating` arm tests overflow via the *biased* exponent of the input float:

```cpp
Value *Cmp3 = Builder.CreateICmpUGE(
    BiasedExp, ConstantInt::getSigned(
                   FloatIntTy, static_cast<int64_t>(ExponentBias +
                                                    BitWidth - IsSigned)));
```

That works as long as the saturation threshold `ExponentBias + BitWidth (- 1)` fits in the FP's exponent field. It does not when the destination is wider than what the FP exponent can represent. For `f32` (max biased exponent `255`) with `i256`, the threshold becomes `127 + 256 = 383`. The biased exponent of `+/- Inf` is `255` (mantissa zero). `255 < 383`, so the `IsSaturating` branch *does not* take the Saturate path. The control flow falls into `CheckExpSizeBB`, then into `ExpLargeBB`, where `Significand = ImplicitBit = 0x800000` is shifted left by `255 - 150 = 105`, producing `2^128` instead of the saturated value (`UINT_MAX` / `SIGNED_MAX` / `SIGNED_MIN`).

The bug also affects:
- `f32 -> iN` for any `N >= 129` (threshold `>= 256 > 255 = max f32 exp`).
- In principle `f16 -> iN` for `N >= 17` (the half fast path mostly hides it for the intrinsic, see w430, but bypassing that path or scenarios where it does fall through hit the same logic).

The non-`IsSaturating` path is fine; this is purely the saturating logic.

## Repro

`.ll` (`/home/orenamd@semianalysis.com/FuzzX/x86/scratch_w430/inf_sat_check.ll`):

```llvm
target triple = "x86_64-unknown-linux-gnu"
define i256 @ui_sat_inf() {
  %r = call i256 @llvm.fptoui.sat.i256.f32(float 0x7FF0000000000000) ; +Inf
  ret i256 %r
}
declare i256 @llvm.fptoui.sat.i256.f32(float)
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu` emits:

```
ui_sat_inf:
    movq    %rdi, %rax
    xorps   %xmm0, %xmm0
    movaps  %xmm0, (%rdi)
    movq    $0, 24(%rdi)
    movq    $1, 16(%rdi)      ; <- the constant '1' written at byte 16 means result == 2^128
    retq
```

The returned i256 (laid out as 4 little-endian qwords) is `0x00..0_00..0_00..1_00..0_00..0` = `2^128`. Expected per LangRef: `UINT_MAX = 2^256 - 1` (all-ones).

A second case (`fptosi.sat`):

```llvm
define i256 @si_sat_inf() {
  %r = call i256 @llvm.fptosi.sat.i256.f32(float 0x7FF0000000000000)
  ret i256 %r
}
define i256 @si_sat_neg_inf() {
  %r = call i256 @llvm.fptosi.sat.i256.f32(float 0xFFF0000000000000) ; -Inf
  ret i256 %r
}
```

Assembly stores `2^128` for `+Inf` (expected `SIGNED_MAX = 2^255 - 1`) and `-2^128` for `-Inf` (expected `SIGNED_MIN = -2^255`).

A smaller case demonstrating the same bug at `i129`:

```llvm
define i129 @ui_sat_inf() {
  %r = call i129 @llvm.fptoui.sat.i129.f32(float 0x7FF0000000000000)
  ret i129 %r
}
```

returns `2^128` instead of `2^129 - 1`.

For reference, `i32` (`/home/orenamd@semianalysis.com/FuzzX/x86/scratch_w430/inf_sat_i32.ll`) — which goes through the normal x86 ISel lowering, *not* this expansion — correctly returns `UINT_MAX` for `+Inf`.

## Why it's wrong

`+/-Inf` is by definition larger in magnitude than any representable finite value, so it must saturate. The check is using the wrong invariant: it asks "is the biased exponent large enough to imply overflow if we computed the integer value?" but never asks "is the input non-finite?" For wide destinations, the threshold is unreachable for the FP source's exponent encoding, so the check is structurally incapable of triggering for Inf.

## Fix sketch

Either (a) add an explicit `Inf` check (`fcmp oeq (fabs x), inf`) into `ZeroResultCond` for the `IsSaturating` arm (with the "saturate-on-Inf" branch routing to `SaturateBB` rather than zero), or (b) clamp the comparison threshold to `min(ExponentBias + BitWidth - IsSigned, MaxBiasedExp)` and add a separate Inf-detector that forces the SaturateBB branch. The simplest correct change is to OR an `is.infinite` test into the predicate that selects `SaturateBB`.

## Candidate-level confidence

High. The assembly is deterministic (no runtime input), the constant `2^128` is plainly visible, and LangRef unambiguously requires saturation to `UINT_MAX` / `SIGNED_{MAX,MIN}` for `+/-Inf`. The bug is independent of optimization level and triggers from the IR expansion pass.
