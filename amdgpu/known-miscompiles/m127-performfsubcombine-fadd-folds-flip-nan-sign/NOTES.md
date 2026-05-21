# m127: `performFSubCombine` `(fsub (fadd a,a), c)` and `(fsub c, (fadd a,a))` folds drop NaN sign without `nnan` guard

*Discovery method: code inspection.*  Sibling shape to m107 (FMul
NaN sign) and m120 (FMul fneg-LHS NaN sign).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:17579-17624`
(`performFSubCombine`).  Two arms, neither gated on FMF beyond
`getFusedOpcode != 0`:

```cpp
// Arm 1 (17596-17608):  (fsub (fadd a,a), c)  ->  fma(a, 2.0, fneg(c))
SDValue NegRHS = DAG.getNode(ISD::FNEG, SL, VT, RHS);
return DAG.getNode(FusedOp, SL, VT, A, Two, NegRHS);

// Arm 2 (17610-17621):  (fsub c, (fadd a,a))  ->  fma(a, -2.0, c)
return DAG.getNode(FusedOp, SL, VT, A, NegTwo, LHS);
```

For non-NaN finite operands both rewrites are bit-exact (`(a+a) - c =
2a - c`, `c - (a+a) = c - 2a`).

For NaN operands the AMDGPU HW NaN-propagation rules diverge:

* HW `v_sub_f32 c, sum` propagates the NaN with the implicit NEG-on-b
  flipping the propagated NaN's sign bit.
* HW `v_fma_f32 a, 2.0, -c` (Arm 1 output, NEG src-modifier on `c`)
  propagates the NaN's sign **unchanged** -- the VOP3 NEG src-modifier
  is XOR'd into the input bits before the FMA, but the FMA's NaN
  propagation rule preserves the resulting sign without further
  manipulation.  Net: original sign-flipped result becomes
  sign-preserved.

Same root cause as m107 and m120, just in a different function.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %out, float %a, float %c) {
  %sum = fadd contract float %a, %a
  %r   = fsub contract float %sum, %c
  store float %r, ptr addrspace(1) %out
  ret void
}
```

Codegen with `clang -mcpu=gfx950`:

```asm
=== -O0 ===
v_add_f32_e64 v1, s3, s3
v_sub_f32_e64 v1, v1, s2   ; HW NEG-on-b flips NaN sign

=== -O2 ===
v_fma_f32 v1, s2, 2.0, -v1 ; VOP3 NEG src-mod doesn't flip NaN sign
```

For `a = 1.0, c = 0x7FC00000` (+qNaN):

* O0: stores `0xFFC00000` (-qNaN -- sign flipped by v_sub).
* O2: stores `0x7FC00000` (+qNaN -- sign preserved by v_fma NEG mod).

Arm 2 (`reduced_arm2.ll` analogously) exhibits the same shape: O0
emits `v_sub v1, v1` then `v_add s3, s3`; O2 emits
`v_fma_f32 v1, -2.0, s2, v1`.  For `a = +qNaN, c = 1.0`: O0 stores
`0xFFC00000`, O2 stores `0x7FC00000`.

## Suggested fix

Gate both arms on `N->getFlags().hasNoNaNs()`:

```cpp
if (!N->getFlags().hasNoNaNs())
  return SDValue();
```

Or compute the FNEG literally (not via NEG src-modifier) for Arm 1.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`v_fma_f32 ..., -v1`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/llc`) | Same fold present (not HEAD-only). |

## Why the fuzzer hasn't caught it

Same as m107: NaN bit-patterns rarely seed `c`-position operands of
`fsub (fadd a,a), c`.  The FP emitter doesn't pair NaN constants with
`contract`-flagged `fadd a,a` chains.

Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
weight `0x7FC00000` and `0xFFC00000` higher in the f32 constant pool
and emit `fsub (contract-fadd a,a) C` / `fsub C (contract-fadd a,a)`
shapes from the random emitter.
