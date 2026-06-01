# 065 — cvt to tf32 with .relu + .satfinite emits qualifiers in wrong order (cvt.rn.relu.satfinite.tf32.f32) — invalid PTX

- **Kind:** other (invalid PTX)
- **Reachable via:** llc -mcpu=sm_100a -mattr=+ptx86
- **Component:** NVPTXInstrInfo.td 705-724 (bug at 723-724); reverse-confirmed by test convert-sm100.ll:31,59  (round-8 area `X12-cvt-sat-relu-arch`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration; no local `ptxas` was available to execute the rejection.

## Summary

`cvt` to tf32 with `.relu`+`.satfinite` emits `cvt.rn.relu.satfinite.tf32.f32` — wrong qualifier order (tf32 grammar requires `.satfinite` before `.relu`; an existing test locks in the wrong output)

## Mechanism / root cause

The CVT_TO_TF32 multiclass builds the instruction mnemonic by literal string substitution: `"cvt." # Modifier # ".tf32.f32"` (line 709). The combined-qualifier instances are declared with Modifier = "rn.relu.satfinite" and "rz.relu.satfinite" (lines 723-724), so the emitted text is `cvt.rn.relu.satfinite.tf32.f32` and `cvt.rz.relu.satfinite.tf32.f32` — `.relu` printed BEFORE `.satfinite`.

This is the wrong PTX qualifier order, exactly the tf32 analog of confirmed bug #038 (add/sub `.sat` before `.ftz`). The PTX ISA fixes the order as satfinite-then-relu specifically for the tf32 cvt. PTX ISA syntax (verified verbatim in releases 8.7, 9.0, 9.2, 9.3):
  cvt.frnd2{.satfinite}{.relu}.tf32.f32   d, a;
so the legal combined form is `cvt.rn.satfinite.relu.tf32.f32`, NOT `cvt.rn.relu.satfinite.tf32.f32`. (Note this differs from the f16/bf16-from-f32 forms `cvt.frnd2{.relu}{.satfinite}.f16.f32`, which are relu-first — LLVM gets those right at lines 574/583/611, but tf32 is the one case PTX flips, and LLVM kept relu-first.) ptxas parses cvt qualifiers positionally and rejects out-of-order qualifiers, so this assembles as invalid PTX. The combination requires sm_100/PTX 8.6 (ISA: 'cvt.{rn/rz}.satfinite.tf32.f32 introduced in PTX ISA version 8.6' / 'requires sm_100 or higher'), matching the LLVM guard [hasPTX<86>, hasSM<100>] — so the instruction itself is target-legal; only the qualifier order is wrong.

The existing regression test llvm/test/CodeGen/NVPTX/convert-sm100.ll (lines 31, 59) locks in the wrong output `cvt.rn.relu.satfinite.tf32.f32` / `cvt.rz.relu.satfinite.tf32.f32`, confirming the order was never checked against the ISA.

## Trigger

Calling @llvm.nvvm.f2tf32.rn.relu.satfinite(float) or @llvm.nvvm.f2tf32.rz.relu.satfinite(float) and codegen for sm_100 with PTX 8.6+. These NVVM builtin intrinsics are declared in IntrinsicsNVVM.td (int_nvvm_f2tf32_{rn,rz}_relu_satfinite, lines 1764-1767).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

declare i32 @llvm.nvvm.f2tf32.rn.relu.satfinite(float)
declare i32 @llvm.nvvm.f2tf32.rz.relu.satfinite(float)

define i32 @t_rn_relu_satf(float %a) {
  %r = call i32 @llvm.nvvm.f2tf32.rn.relu.satfinite(float %a)
  ret i32 %r
}
define i32 @t_rz_relu_satf(float %a) {
  %r = call i32 @llvm.nvvm.f2tf32.rz.relu.satfinite(float %a)
  ret i32 %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_100a -mattr=+ptx86 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.85). 
