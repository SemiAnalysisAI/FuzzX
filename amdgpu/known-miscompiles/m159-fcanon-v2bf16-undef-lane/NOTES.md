# m159: `performFCanonicalizeCombine` v2bf16 undef-lane handling missing (sibling of m115/m124 for bf16)

*Discovery method: random fcanon-chain IR fuzzing; 29/500 mismatches all
of this shape on real gfx950 HW.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:15885`
(`performFCanonicalizeCombine`):

The BUILD_VECTOR per-lane undef-fixup is guarded by:

```cpp
if (VT == MVT::v2f16) {
  // ... fixup undef lanes ...
}
```

**v2bf16 is missing**.  On gfx950, v2bf16 fcanonicalize is Legal
(see `SIISelLowering.cpp:806, 1004`), so v2bf16 BUILD_VECTOR with
an undef lane falls through this guard and is left as-is.

At O0 the undef lane reads garbage register bits via
`v_max_f32_e64` (or the bf16 variant) with implicit-def operand.
At O2 a constant qNaN gets splatted via earlier folds.  The
divergence is concrete and reproducible on HW.

Example reproducer input `0x00007fc0` (lane0 undef, lane1 = bf16
qNaN):

* O0 stores `0x7fc00000`
* O2 stores `0x7fc07fc0`

Structurally m115/m124 but for the bf16-packed type, which is
legal on gfx950 and uses the same Custom-lower path.

## Reproducer

`reduced.ll` (from random fuzz, kept verbatim).  Runs through
`amdgpu/known-miscompiles/run_ll_reproducer.sh` and produces
deterministic O0/O2 divergence on `RUN-INPUTS: 0x00007fc0`.

## Suggested fix

Extend the guard at `SIISelLowering.cpp:15885` to include v2bf16:

```cpp
if (VT == MVT::v2f16 || VT == MVT::v2bf16) {
  // ... per-lane undef fixup ...
}
```

Or factor out the v2-of-half-width lane fixup to a helper called
for any 2-lane half-precision VT (v2f16, v2bf16).

## Why the fuzzer caught it

* The new fcanon random emitter biased toward `<2 x bfloat>` with
  per-lane undef seeded via `extractelement` / `insertelement`
  chains with `poison` placeholders, plus NaN/Inf/-0/denormal bit
  pools.
* The differential O0/O2 oracle compares stored i32 bit pattern
  exactly -- undef-lane garbage is observable.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | 29/500 mismatches in 12-min fuzz; deterministic on saved repro. |
| ROCm 7.1.1 | Same defect. |

## Family

* m115 / m124 (v2f16 fcanonicalize undef-lane) -- same defect,
  different element type.
* m141 (isCanonicalized bitcast loses fp-type).
* m118 / m133 / m147 (other fcanon-family defects).

## Adjacent runtime-confirmed shape (not separately filed)

Same fuzz batch found `s_bitcast_v2bf16_v2f16` shape produces 4
additional O0/O2 mismatches.  Root cause is m141 (isCanonicalized
recursion through BITCAST losing fp-type).  m141's NOTES document
this; the runtime-observable instance lives there.
