# m129: `performFNegCombine` FCANONICALIZE arm flips NaN sign at O2

*Discovery method: code inspection.*  Sibling shape to
m107/m110/m111/m120/m127/m128 (SDAG NaN-sign-flip family).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:5402-5427`
(`performFNegCombine`, case `ISD::FCANONICALIZE` lumped with
`FP_EXTEND`/`FTRUNC`/`FRINT`/etc.):

```cpp
case ISD::FCANONICALIZE: /* and 7 sibling opcodes */ {
  SDValue CvtSrc = N0.getOperand(0);
  if (CvtSrc.getOpcode() == ISD::FNEG)               // (B2) collapse
    return DAG.getNode(Opc, SL, VT, CvtSrc.getOperand(0));
  ...
  // (B1) push fneg inside
  SDValue Neg = DAG.getNode(ISD::FNEG, SL, CvtSrc.getValueType(), CvtSrc);
  return DAG.getNode(Opc, SL, VT, Neg, N0->getFlags());
}
```

The combine fires unconditionally -- no `nnan` gate.
`fnegFoldsIntoOpcode` whitelists `FCANONICALIZE` at line 684.  Two
rewrites:

* **B1**: `fneg(fcanon x)` -> `fcanon(fneg x)`
* **B2**: `fneg(fcanon(fneg x))` -> `fcanon x` (double-fneg collapse)

On gfx950, `fcanonicalize` lowers to `v_max_f32 x, x`.  AMDGPU HW
canonicalizes any NaN to a **positive** qNaN, regardless of the input
NaN's sign bit:

| input | `v_max_f32(x, x)` | IR `fcanonicalize(x)` |
| --- | --- | --- |
| `-qNaN` | `+qNaN` | `+qNaN` (matches HW) |
| `+qNaN` | `+qNaN` | `+qNaN` |

But IR `fneg(fcanonicalize(-qNaN)) = fneg(+qNaN) = -qNaN` (LangRef
fneg is unconditional sign-bit flip).

* O0 (no combine): `v_max(x,x)` -> `+qNaN`, then `v_sub(-0, +qNaN)` ->
  `-qNaN` (HW NEG-modifier flips NaN sign on a v_sub).
* O2 (fold fires, B1): `v_max(-x, -x)` reads `-(-qNaN) = +qNaN` via
  NEG src-modifier, then `v_max(+qNaN, +qNaN) = +qNaN`.  Result is
  `+qNaN` -- **sign NOT flipped**.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %out, float %x) {
  %c = call float @llvm.canonicalize.f32(float %x)
  %n = fsub float -0.0, %c          ; canonical fneg
  store float %n, ptr addrspace(1) %out
  ret void
}
```

Codegen with `clang -mcpu=gfx950`:

```asm
=== -O0 ===
v_max_f32_e64 v1, s2, s2          ; fcanon -> +qNaN
v_sub_f32_e64 v1, 0x80000000, v1  ; -0 - +qNaN -> -qNaN (HW NEG-on-b)

=== -O2 ===
v_max_f32_e64 v1, -s2, -s2        ; folded fcanon(fneg x); -(-qNaN)=+qNaN
                                  ;   then v_max(+qNaN,+qNaN) = +qNaN
```

For `x = 0xFFC00000` (-qNaN):

* O0 stores `0xFFC00000` (-qNaN).
* O2 stores `0x7FC00000` (+qNaN).

Same divergence for `fneg(fcanon(fneg x))` (B2 collapse) under
analogous setup.

## Suggested fix

Split the `FCANONICALIZE` case out of the bulk FP-conversion arm and
gate on `nnan`:

```cpp
case ISD::FCANONICALIZE: {
  if (!N->getFlags().hasNoNaNs() && !N0->getFlags().hasNoNaNs())
    return SDValue();    // HW fcanon NaN -> +qNaN; can't soundly invert
  ...
}
```

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same fold present. |

## Why the fuzzer hasn't caught it

Same as the rest of the NaN-sign-flip family: NaN bit-patterns rarely
seed FP operand positions of `fneg(llvm.canonicalize(x))`.
Per `MEMORY.md` (Prefer-random-over-idioms), weight `0x7FC00000`,
`0xFFC00000`, `0x7F800001` higher in the f32 constant pool and let
the existing emitter feed them into any `llvm.canonicalize.f32`
consumer.
