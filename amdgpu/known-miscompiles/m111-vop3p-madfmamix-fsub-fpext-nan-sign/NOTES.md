# m111: VOP3P `MadFmaMix` TableGen pattern `(fsub x, fpext h) -> v_fma_mix(h, -1.0, x)` flips NaN sign at -O0

*Discovery method: code inspection.*  Sibling shape to m107 (FMUL NaN
sign flip) but in a TableGen pattern rather than a SDAG combine.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/VOP3PInstructions.td:240-251`
defines the `MadFmaMixFP32Pats` rewrite:

```td
def : GCNPat <
  (fsub (VOP3PMadMixModsExt0 f32:$src0, i32:$src0_mod),
        (fpround (fmul (VOP3PMadMixModsExt2 f16:$src1, i32:$src1_mod), ...))),
  ...>;
```

(and adjacent shapes that include `(fsub x, fpext y)`).

This matches the canonical IR shape of `fneg(fpext h)` -- which lowers
through generic IR builder as `fsub -0.0, fpext(h)` -- and emits
`v_fma_mix_f32(h, -1.0, -0.0)` with appropriate `op_sel_hi`.

The VOP3 NEG source modifier applied to `-1.0` does not flip the sign
bit of a NaN propagated from `h`: HW `v_fma_mix(NaN, -1, -0)` returns
`+NaN` (the input NaN's sign bit is preserved).

So:

| pipeline | codegen | result for `fneg(fpext +qNaN_half)` |
| --- | --- | --- |
| O0 (TableGen pattern wins) | `v_fma_mix_f32 v1, h, -1.0, -0.0` | `0x7FC00000` (+qNaN) |
| O2 (FNeg combine fires first) | `v_cvt_f32_f16_e64 v1, -h` | `0xFFC00000` (-qNaN) |

LangRef `fneg` semantics require the sign bit to be flipped for every
value including NaN.  The O0 codegen violates this.

The `performFNegCombine` FP_EXTEND arm
(`AMDGPUISelLowering.cpp:5402-5427`) is **itself** fine: it folds
`fneg(fpext h) -> fpext(fneg h)`, which lowers to `v_cvt_f32_f16(-h)`.
That correctly negates the half before extension, preserving the
(flipped) sign through the conversion.

The bug is that at `-O0`, DAGCombiner does NOT run, so the FNeg combine
never fires and the FMA-mix TableGen pattern wins instead.  At `-O2`
the FNeg combine runs first and the FMA-mix pattern doesn't match.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %out, half %x) {
  %ext = fpext half %x to float
  %r   = fsub float -0.0, %ext       ; canonical fneg(fpext h)
  store float %r, ptr addrspace(1) %out
  ret void
}
```

For `x = +qNaN_half (0x7E00)`:

* O0: stores `0x7FC00000` (+qNaN -- sign NOT flipped, **wrong**).
* O2: stores `0xFFC00000` (-qNaN -- sign flipped, correct).

## Suggested fix

Add `nnan` gates to the `MadFmaMixFP32Pats` arms that match the
`(fsub x, fpext y)` shape.  Or, more permissively, restrict the
pattern to non-`-0.0` LHS (so canonical `fneg`-shaped IR doesn't
match):

```td
def : GCNPat <
  (fsub (VOP3PMadMixModsExt0 f32:$src0, i32:$src0_mod) /* not -0.0 */,
        (fpround ...)),
  ...>;
```

A simpler fix is to apply the FNeg-FPExt combine at all opt levels
(including -O0) by promoting it to a DAG-builder canonicalisation
rather than a DAGCombine.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces at O0 (`v_fma_mix_f32`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Does NOT reproduce -- the FMA-mix pattern was not yet present or had different gating. **HEAD-only regression at O0.** |

## Why the fuzzer hasn't caught it

* The harness compiles + runs at both O0 and O2, but the O0-vs-O2
  diff for half FP arithmetic rarely lands on this specific
  `fneg(fpext NaN_half)` shape.
* The interpreter oracle currently skips f16-arithmetic kernels with
  NaN inputs.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  weight `0x7E00` (qNaN half) higher in the f16 constant pool and
  ensure `fneg(fpext h)` patterns are emitted by the random IR fuzzer.
