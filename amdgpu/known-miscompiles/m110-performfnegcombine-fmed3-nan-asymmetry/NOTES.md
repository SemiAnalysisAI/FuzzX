# m110: `performFNegCombine` FMED3 arm folds `fneg(fmed3 x,y,z) -> fmed3(-x,-y,-z)` -- wrong when an operand is NaN

*Discovery method: code inspection.*  Sibling shape to m107 (FMUL NaN sign
flip) and m092/m095 (NaN/sign-of-zero family).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:5383-5401`
(`AMDGPUTargetLowering::performFNegCombine`, case `AMDGPUISD::FMED3`):

```cpp
case AMDGPUISD::FMED3: {
  ...
  SDValue NegX = DAG.getNode(ISD::FNEG, SL, VT, N0->getOperand(0));
  SDValue NegY = DAG.getNode(ISD::FNEG, SL, VT, N0->getOperand(1));
  SDValue NegZ = DAG.getNode(ISD::FNEG, SL, VT, N0->getOperand(2));
  return DAG.getNode(AMDGPUISD::FMED3, SL, VT, NegX, NegY, NegZ);
}
```

The rewrite has NO `nnan` guard.  It is value-correct only when no
operand can be NaN.

`v_med3_f32` treats NaN asymmetrically -- NaN sorts as "smaller than
everything else", regardless of the NaN's sign bit.  Concretely:

| inputs | median | rationale |
| --- | --- | --- |
| `med3(NaN, 1.0, 2.0)` | `1.0` | NaN smallest, sorted: `[NaN, 1, 2]`, median = `1`. |
| `med3(-NaN, -1.0, -2.0)` | `-2.0` | `-NaN` smallest, sorted: `[-NaN, -2, -1]`, median = `-2`. |

So `-med3(NaN, 1.0, 2.0) = -1.0` but
`med3(-NaN, -1.0, -2.0) = -2.0`.  Negating the operands does NOT yield
a sign-flipped median.

## Reproducer

`reduced.ll`:

```llvm
declare float @llvm.amdgcn.fmed3.f32(float, float, float)

define amdgpu_kernel void @t(ptr addrspace(1) %out, float %x, float %y, float %z) {
  %m = call float @llvm.amdgcn.fmed3.f32(float %x, float %y, float %z)
  %r = fsub float -0.0, %m
  store float %r, ptr addrspace(1) %out
  ret void
}
```

Codegen with `clang -mcpu=gfx950`:

```asm
=== -O0 ===
v_med3_f32 v1, v1, v2, v3        ; med3(NaN, 1, 2) = 1.0
v_sub_f32_e64 v1, 0x80000000, v1 ; -0 - 1.0 = -1.0

=== -O2 ===
v_med3_f32 v1, -v1, -v2, -v3     ; med3(-NaN, -1, -2) = -2.0
```

For `x = +qNaN (0x7FC00000), y = 1.0, z = 2.0`:

* O0 stores `0xBF800000` (-1.0).
* O2 stores `0xC0000000` (-2.0).

Verified mismatch on three different NaN-position orderings (NaN as x,
y, or z).  Reproduces on ROCm 7.1.1 -- **not HEAD-only.**

## Suggested fix

Gate on `nnan`:

```cpp
case AMDGPUISD::FMED3: {
  if (!N->getFlags().hasNoNaNs())
    break;
  ...
}
```

Or, more tightly, only fold when at least one operand is
`isKnownNeverNaN` and the other two operands are negated.  But the
`nnan` gate is the simpler / safer fix.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`v_med3_f32 -v, -v, -v`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same fold present (not HEAD-only). |

## Why the fuzzer hasn't caught it

* The FP emitter generates `fneg(fmed3 ...)` shapes but the operand
  pool rarely seeds NaN bit-patterns in arithmetic positions.
* The interpreter oracle skips kernels using `llvm.amdgcn.fmed3.f32`
  (target intrinsic).
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  weight NaN bit-patterns higher in the f32 constant pool and ensure
  the random emitter can produce `fneg(fmed3 ...)` patterns -- the
  bad fold will then surface naturally.

## Related: other FNeg arms

* FMUL/FMUL_LEGACY/FADD/FMA (5298-5318): already filed as m107 (same
  NaN-sign flip issue).
* FCANONICALIZE (5408): runtime-tested, OK.
* FP_ROUND (5428-5443): runtime-tested, OK.
* FP_EXTEND (5402-5427) itself: OK; but the related VOP3P MadFmaMix
  TableGen pattern races it at O0 -- separate bug (m111).
* FMAXIMUM/FMAXNUM (5349-5381): symmetric negation, safe.
