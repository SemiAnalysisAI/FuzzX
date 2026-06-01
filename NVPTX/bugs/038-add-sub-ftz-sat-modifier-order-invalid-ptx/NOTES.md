# 038 — f32 nvvm add/sub with ftz+sat emit invalid modifier order `add.rn.sat.ftz.f32` / `sub.rn.sat.ftz.f32` (.sat before .ftz)

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc
- **Component:** NVPTXIntrinsics.td 1888, 1892, 1896, 1900 (add); 1957 (sub synthesis)  (round-6 area `W06-math-approx`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + strong in-tree corroboration (sibling guards / orderings); no local `ptxas` was available to execute the rejection.

## Summary

f32 `nvvm.add/sub.*.ftz.sat` emit `add.rn.sat.ftz.f32` (`.sat` before `.ftz`) — wrong PTX modifier order (f16 add, f32 fma all use the correct `.ftz.sat`)

## Mechanism / root cause

The PTX ISA grammar for single-precision add/sub is `add{.rnd}{.ftz}{.sat}.f32` and `sub{.rnd}{.ftz}{.sat}.f32` — `.ftz` MUST precede `.sat`. But the f32 saturating-ftz add intrinsics hard-code the asm string with the modifiers swapped:
  line 1888: `F_MATH_2<"add.rn.sat.ftz.f32", ...>` (for int_nvvm_add_rn_ftz_sat_f)
  lines 1892/1896/1900: same swap for rz/rm/rp.
The f32 sub path synthesizes the string at line 1957:
  `!subst("_", ".", "sub" # rnd # sat # ftz # "_f32")`  -> `sub` then `sat` then `ftz`, yielding `sub.rn.sat.ftz.f32` for the ftz+sat combo. (Note the def NAME on line 1955 uses the correct `rnd # ftz # sat` order, but the emitted asm string on line 1957 swaps them — clearly a typo, not intent.)
This is provably a bug (not spec-lawyering) because the SAME file emits the CORRECT `.ftz.sat` order for every analogous case: f16 add `add.rn.ftz.sat.f16` (line 1883), f16 mul `mul.rn.ftz.sat.f16` (line 1511), and f32 fma `fma.rn.ftz.sat.f32` (FMA_TUPLE `_rn_ftz_sat_f32`, line 1688). Only f32 add and f32 sub have `.sat` and `.ftz` transposed. ptxas enforces the fixed modifier grammar order and rejects `add.rn.sat.ftz.f32`/`sub.rn.sat.ftz.f32` (unrecognized instruction form), so the backend emits unassemblable PTX for valid IR on a fully supported target.

## Trigger

Call llvm.nvvm.add.{rn,rz,rm,rp}.ftz.sat.f (and the synthesized f32 sub via add(a, fneg(b))). Any sm/PTX target. e.g. `%r = call float @llvm.nvvm.add.rn.ftz.sat.f(float %a, float %b)` emits `add.rn.sat.ftz.f32`.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

declare float @llvm.nvvm.add.rn.ftz.sat.f(float, float)
declare float @llvm.nvvm.add.rz.ftz.sat.f(float, float)

define float @add_rn_ftz_sat(float %a, float %b) {
  %r = call float @llvm.nvvm.add.rn.ftz.sat.f(float %a, float %b)
  ret float %r
}

define float @sub_rn_ftz_sat(float %a, float %b) {
  %nb = fneg float %b
  %r = call float @llvm.nvvm.add.rn.ftz.sat.f(float %a, float %nb)
  ret float %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_80 -o - repro.ll`

## Verification

Reproduced with the built llc (emitted PTX / crash matches the claim; finder confidence 0.83, confirmed_with_llc=True).
