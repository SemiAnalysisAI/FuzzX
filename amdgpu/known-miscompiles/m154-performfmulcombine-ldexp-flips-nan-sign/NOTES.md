# m154: `performFMulCombine` ldexp rewrite flips NaN sign via FNEG src-modifier

*Discovery method: code inspection (during amdgcn.fract/frexp/ldexp audit).*

Sibling of m107 (FMUL arm), m120 (FMul fneg-LHS).  Same family: a
combine that moves a sign-flip into a VOP3 src-modifier on an operand
of an op whose NaN output sign equals the input sign, without `nnan`
guard.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:17684-17722`
(`SITargetLowering::performFMulCombine`, ldexp arm):

```cpp
// fmul x, (select c, A, B) -> ldexp(x, select c, log2|A|, log2|B|)
// fmul x, (select c, -A, -B) -> ldexp(fneg x, select c, log2|A|, log2|B|)
if (TrueNode && FalseNode &&
    TrueNode->isNegative() == FalseNode->isNegative()) {
  ...
  if (TrueNode->isNegative())
    LHS = DAG.getNode(ISD::FNEG, DL, VT, LHS);    // <-- NaN sign flipped here
  return DAG.getNode(ISD::FLDEXP, DL, VT, LHS, Sel);
}
```

Gate is only `TrueNode->isNegative() == FalseNode->isNegative()` +
exact-log2.  **NO check on `N->getFlags().hasNoNaNs()`.**

For `x = NaN`:

* Original: `v_mul_f32(NaN, -K)` -- output NaN sign == input NaN sign
  (per m107: HW v_mul propagates input NaN sign unchanged through the
  negative scalar).
* Rewrite: `v_ldexp_f32(-NaN, exp)` -- the VOP3 `-` src-modifier XORs
  the sign bit before ldexp; `v_ldexp_f32` then passes the NaN
  through unchanged (per AMD ISA "Result of LDEXP" passthrough).
  Output NaN sign **flipped**.

Applies to **f16, f32, f64** -- all three handled by the combine
(gated at line 17685).

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %in, ptr addrspace(1) %out, i1 %c) {
  %x = load float, ptr addrspace(1) %in
  %s = select i1 %c, float -4.0, float -8.0
  %r = fmul float %x, %s
  store float %r, ptr addrspace(1) %out
  ret void
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll` emits:

```asm
v_cndmask_b32_e64 v1, 3, 2, vcc       ; select between log2(4) and log2(8)
v_ldexp_f32 v0, -v0, v1               ; VOP3 NEG on src0; ldexp NaN sign flipped
```

For `x = +qNaN(0x7FC00000)`, output is `-qNaN(0xFFC00000)`.

Baseline (without the combine, at O0 or with `nnan` blocking):
`v_mul_f32(x, -4.0)` returns `+qNaN(0x7FC00000)`.

## Suggested fix

Add `nnan` gate at line 17685:

```cpp
if (TrueNode && FalseNode &&
    TrueNode->isNegative() == FalseNode->isNegative() &&
    N->getFlags().hasNoNaNs()) {                  // <-- ADD
  ...
}
```

Same fix shape as m107/m120/m127/m139/m140.  An audit of the
entire `performFMulCombine` for `nnan`-missing `nsz`-only-or-no-FMF
rewrites should be folded into one PR.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Combine fires; NaN sign flips. |
| ROCm 7.1.1 | Same defect. |

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely produces `fmul x, (select c, -K1, -K2)` with
  K1/K2 exact powers of 2.  Per `MEMORY.md`
  (Prefer-random-over-idioms), the random emitter should bias toward
  negative-power-of-2 select arms.
* The differential O0-vs-O2 oracle catches the NaN-sign flip if the
  oracle compares bit patterns.

## Family

* m107 (FMUL arm) -- "the" original.
* m110 (FMED3 arm).
* m111 (VOP3P MadFmaMix TableGen pattern).
* m120 (FMul fneg-LHS).
* m127 (FSub fadd folds).
* m128 (FDOT2).
* m139 (FMA arm).
* m140 (FADD arm).
* m154 (FMul ldexp arm) -- this entry.

Same root cause across all: a combine inserting an FNEG
src-modifier on an arithmetic operand whose NaN output sign would
otherwise be preserved.
