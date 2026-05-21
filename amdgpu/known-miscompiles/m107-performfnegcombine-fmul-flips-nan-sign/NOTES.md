# m107: `performFNegCombine` FMUL arm folds `fneg(fmul x, y)` into VOP3 NEG src-modifier, flipping the sign of a propagated NaN

*Discovery method: code inspection.*  Sibling shape to m092 (`select
(fcmp one x, K), other, K` NaN-arg) and m094 (`fmul.legacy` sign of
zero), but operates on real `fneg`-of-`fmul` chains rather than the
`fmul.legacy` intrinsic.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:5298-5318`
(`AMDGPUTargetLowering::performFNegCombine`, case `ISD::FMUL` /
`AMDGPUISD::FMUL_LEGACY`):

```cpp
case ISD::FMUL:
case AMDGPUISD::FMUL_LEGACY: {
  ...
  // (fneg (fmul x, y)) -> (fmul x, (fneg y))
  ...
  SDValue NegRHS = DAG.getNode(ISD::FNEG, SL, VT, RHS);
  SDValue Res = DAG.getNode(N0Opc, SL, VT, LHS, NegRHS, N0->getFlags());
  ...
}
```

The rewrite has NO `nnan` guard.  The `FADD` arm (5273-5297) and the
`FMA` arm (5322) have an `nsz` guard but neither has `nnan`.

Under IEEE 754, `fneg(x) = x XOR 0x80000000` for every `x` including
NaN, so the sign bit of a NaN is **always** flipped by `fneg`.

But on AMDGPU HW, `v_mul_f32(NaN, -y)` propagates the **input NaN's
sign bit** unchanged -- the VOP3 NEG source modifier on the *other*
operand has no effect on a propagated NaN's sign.

Consequence: for any `x` that is NaN at runtime, the original IR
`fneg(fmul x, y)` produces a NaN with the sign bit XOR'd, while the
folded `fmul(x, -y)` produces a NaN with the input sign bit unchanged.

The same issue applies to:

* `ISD::FADD` arm at line 5274 (when `nsz` is set but `nnan` isn't):
  HW `v_add_f32(NaN, -y)` similarly preserves NaN sign.
* `ISD::FMA` arm at line 5322 (same shape, `nsz`-only gate).

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %out, float %x, float %y) {
  %m = fmul float %x, %y
  %n = fsub float -0.0, %m       ; canonical IR `fneg(m)`
  store float %n, ptr addrspace(1) %out
  ret void
}
```

Codegen with `clang -mcpu=gfx950`:

```asm
=== -O0 ===
v_mul_f32_e64 v1, s2, v1
v_sub_f32_e64 v1, 0x80000000, v1     ; honors fsub: -0 - mul

=== -O2 ===
v_mul_f32_e64 v1, s2, -v1            ; fneg folded into NEG modifier
```

For `x = 0x7FC00000 (+qNaN), y = 1.0`:

* O0: `v_mul(NaN,1) = +NaN`; `v_sub(-0, +NaN)` returns `-NaN`
  (`v_sub` is `a + (-b)`; HW negates the NaN's sign bit on the source
  modifier of operand b).
* O2: `v_mul(NaN, -1)` returns `+NaN` (the NEG src-modifier doesn't
  affect a propagated NaN's sign).

Result: O0 stores `0xFFC00000`, O2 stores `0x7FC00000`.

## Suggested fix

Gate the FMUL/FADD/FMA arms on `nnan`.  Either:

```cpp
case ISD::FMUL:
case AMDGPUISD::FMUL_LEGACY:
  if (!N->getFlags().hasNoNaNs())
    break;
  ...
```

or, more tightly, the existing `nsz` guard could be extended to
`nsz & nnan`.

## Why no runtime O0/O2 mismatch in the harness for non-NaN inputs

For finite non-NaN `x` and `y`, the two forms produce the same value
because for finite NaN-free `a, b`: `-a*b = a*(-b)` exactly.  The
bug surfaces only when `x` (or `y`, depending on which side the fneg
folds into) is NaN at runtime.  The FuzzX harness's current FP
inputs rarely seed NaN values in arithmetic positions.

The asm divergence is observable at every `fneg(fmul x, y)` site
regardless of input values, so the bug is reachable from any source
that emits `-a*b`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`v_mul_f32_e64 v1, s2, -v1`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same fold present (`performFNegCombine` unchanged). |

Not a HEAD-only regression.

## Why the fuzzer hasn't caught it

* The current FP emitter generates `fneg(fmul x, y)` shapes plenty,
  but rarely with `x` simultaneously being a NaN bit-pattern.
* The interpreter oracle currently skips NaN inputs to FP intrinsics
  outside the `llvm.canonicalize` family.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  weight `0x7FC00000` (qNaN), `0xFFC00000` (-qNaN), and
  `0x7F800001` (sNaN) higher in the random f32 constant pool and
  ensure `fneg(fmul x, y)` patterns can pick them as `x`.
