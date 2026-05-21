# m148: `(v2f16 (fpround v2f64:$src))` GCNPat double-rounds; scalar f64 -> f16 single-rounds

*Discovery method: code inspection (during FP_ROUND audit).*  Direct
sibling of m137 (`LowerF64ToF16Safe` NaN payload divergence).  Same
IR, same gfx950: scalar `fptrunc double to half` and lane-0 of
`fptrunc <2 x double> to <2 x half>` produce different f16 results
for f64 values near half-way f16 boundaries.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/VOP3Instructions.td:1461-1463`:

```tablegen
def : GCNPat <
  (v2f16 (fpround v2f64:$src)),
  (V_CVT_PK_F16_F32_e64 0, (V_CVT_F32_F64_e32 (EXTRACT_SUBREG $src, sub0)),
                        0, (V_CVT_F32_F64_e32 (EXTRACT_SUBREG $src, sub1)))
>;
```

This emits **double-rounding**: f64 -> f32 (via `V_CVT_F32_F64`)
followed by f32 -> f16 (via `V_CVT_PK_F16_F32`).  Classic IEEE
double-rounding loses 1 ULP for values that lie on a half-way
boundary between two f16 numbers when the intermediate f32 falls on
the opposite half.

The scalar path (`SIISelLowering.cpp:8599` ->
`AMDGPUTargetLowering::LowerF64ToF16Safe`,
`AMDGPUISelLowering.cpp:3787-3873`) is designed to emulate
**single-step** rounding via an inexact-to-odd correction sequence.

Same IR, same gfx950, divergent f16 bits depending on whether the
fptrunc is scalar (uses `LowerF64ToF16Safe`) or vector (uses the
GCNPat).

## Reproducer

`reduced.ll` stores the scalar result and the lane-0 vector result
side-by-side in one i32 (low half = scalar, high half = vector
lane 0):

```llvm
%scalar_h = fptrunc double %xd to half
%v        = insertelement <2 x double> poison, double %xd, i32 0
%v2       = insertelement <2 x double> %v,     double %xd, i32 1
%vh       = fptrunc <2 x double> %v2 to <2 x half>
%vec_h    = extractelement <2 x half> %vh, i32 0
; ... pack into i32 store
```

For half-way-boundary f64 inputs (e.g. `0x3F1000000001FFFF`),
`%scalar_h` and `%vec_h` differ by 1 ULP within the same kernel.

The bug also propagates NaN-payload differently: the vector GCNPat
skips the m137 NaN-payload-quietening logic in `LowerF64ToF16Safe`,
so NaN bit patterns round-trip differently between the two paths.

## Suggested fix

Replace the GCNPat at `VOP3Instructions.td:1461-1463` with a custom
lowering that scalarises to two `LowerF64ToF16Safe` calls then
packs via `V_CVT_PK_F16_F32` (or directly into a v2f16 register).
Alternatively, extend `LowerF64ToF16Safe` to handle v2f64 source
type and have `lowerFP_ROUND` route v2f64->v2f16 through it instead
of returning `SDValue()` and letting the GCNPat fire.

In `SIISelLowering.cpp` `lowerFP_ROUND`, the v2f16-dst branch at
line 8573 currently bails on non-f32 source:

```cpp
if (DstVT.isVector() && DstVT.getScalarType() == MVT::f16 &&
    SrcVT.getScalarType() != MVT::f32)
  return SDValue();   // <-- bails; the GCNPat then fires for v2f64
```

The fix is to handle the v2f64 source case here.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely generates side-by-side scalar/vector fptrunc
  of the same f64 value with a half-way boundary input.  Per
  `MEMORY.md` (Prefer-random-over-idioms), the random emitter should
  bias toward f64 inputs with mantissa patterns near `0x...001FFFF`
  / `0x...0020000` (the half-way boundaries) and emit both scalar
  and vector fptrunc paths.
* The differential O0-vs-O2 oracle won't catch this because both
  opt levels share the same GCNPat for vector fpround.  The oracle
  needs to be augmented to compare scalar-fptrunc vs
  insert-into-vector-then-fptrunc-vector.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | GCNPat fires; double-rounding observed. |
| ROCm 7.1.1 | Same defect. |

## Family

* m137 (`LowerF64ToF16Safe` NaN payload divergence) -- same scalar
  function; m148 is the *vector* path completely bypassing it.
* m143 (STRICT_FP_ROUND f64->bf16 drops chain) -- same area
  (lowerFP_ROUND), different defect.
* m141 (`isCanonicalized` recurses through BITCAST losing FP-type).
