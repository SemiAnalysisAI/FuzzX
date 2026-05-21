# m128: `performFMACombine` FDOT2 fold flips NaN sign -- `v_dot2c_f32_f16` forces sign=1 on any NaN output

*Discovery method: code inspection + runtime check on MI300.*  Sibling
shape to m107/m110/m111/m120/m127 (SDAG NaN-sign-flip family) and m100
(same fold, different bit of FP semantics: m100 = denormal, m128 = NaN
sign).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:17729-17800`
(`performFMACombine`) folds the FMA-chain pattern
`fma(fpext(a.x), fpext(b.x), fma(fpext(a.y), fpext(b.y), z))` into
`AMDGPUISD::FDOT2`, gated only on `contract` and the `dot10-insts`
feature.  **No `nnan` guard** on either FMA's flags.

The source IR lowers to two `v_fma_mix_f32` ops.  Folded form is one
`v_dot2c_f32_f16_e32`.  AMDGPU HW NaN-propagation differs between the
two:

* `v_fma_mix_f32` propagates the input NaN's sign and payload per the
  AMDGPU FMA NaN rules.
* `v_dot2c_f32_f16` **unconditionally sets the sign bit** of any NaN
  output, regardless of input NaN sign or which operand is NaN.  The
  accumulate step in particular forces sign=1 on any NaN that propagates
  through `z`.

LangRef `contract` is a fusion permit; it does NOT authorise NaN-sign
loss.  That would require `nnan`.

## Reproducer

`reduced.ll`:

```llvm
declare float @llvm.fma.f32(float, float, float)

define amdgpu_kernel void @t(ptr addrspace(1) %out, <2 x half> %a, <2 x half> %b, float %z) {
  %ax = extractelement <2 x half> %a, i32 0
  %ay = extractelement <2 x half> %a, i32 1
  %bx = extractelement <2 x half> %b, i32 0
  %by = extractelement <2 x half> %b, i32 1
  %axf = fpext half %ax to float
  %ayf = fpext half %ay to float
  %bxf = fpext half %bx to float
  %byf = fpext half %by to float
  %inner = call contract float @llvm.fma.f32(float %ayf, float %byf, float %z)
  %outer = call contract float @llvm.fma.f32(float %axf, float %bxf, float %inner)
  store float %outer, ptr addrspace(1) %out
  ret void
}
```

Runtime on MI300 with `hip_module_runner`:

| inputs | `+dot10-insts` (FDOT2) | `-dot10-insts` (two v_fma_mix_f32) |
| --- | --- | --- |
| `a=<+qNaN, 1.0>, b=<1,1>, z=0`   | `0xFFC00000` (-qNaN) | `0x7FC00000` (+qNaN) |
| `a=<1.0, +qNaN>, b=<1,1>, z=0`   | `0xFFC00000` (-qNaN) | `0x7FC00000` (+qNaN) |
| `a=<-qNaN, 1.0>, b=<1,1>, z=0`   | `0xFFC00000` (-qNaN) | `0xFFC00000` (-qNaN, match) |
| `a=<1, 1>, b=<1, 1>, z=+qNaN`    | `0xFFC00000` (-qNaN) | `0x7FC00000` (+qNaN) |

The bug fires even when the NaN flows through `z` only -- the
`v_dot2c_f32_f16` accumulate step forces sign=1 on the output NaN.

## Suggested fix

Add `nnan` to the gate at line 17752-17754 alongside the existing
`contract` gate (and the m100 denormal-mode gate):

```cpp
if (!(Options.AllowFPOpFusion == FPOpFusion::Fast) &&
    !(N->getFlags().hasAllowContract() &&
      FMA->getFlags().hasAllowContract()))
  return SDValue();
if (!N->getFlags().hasNoNaNs() || !FMA->getFlags().hasNoNaNs())
  return SDValue();   // FDOT2 forces sign=1 on NaN outputs
```

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same fold, same NaN-sign flip. |

## Why the fuzzer hasn't caught it

* Same as m107/m120/m127: NaN bit-patterns rarely seed FP operand
  positions, and the `fpext(extract <2xhalf>) * fpext(extract <2xhalf>)`
  chain in the `contract`-FMA shape is a narrow corpus target.
* The current FuzzX harness's runtime oracle does not distinguish
  +qNaN from -qNaN.
* Per `MEMORY.md` (Prefer-random-over-idioms), weight `0x7E00` /
  `0xFE00` higher in the f16 constant pool and bias the random
  emitter toward `extract_vector_elt` + `fpext` + paired `contract`
  FMA chains.
