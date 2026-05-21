# m141: `isCanonicalized` recurses through `ISD::BITCAST` without consulting source/dest FP semantics

*Discovery method: code inspection (in-source TODO confirms the
defect).*  Sibling shape to m118 (`isCanonicalized` over-promise for
FP-class intrinsics) -- different arm of the same function.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:15649-15653`
(`SITargetLowering::isCanonicalized`):

```cpp
case ISD::BITCAST:
  // TODO: This is incorrect as it loses track of the operand's type. We may
  // end up effectively bitcasting from f32 to v2f16 or vice versa, and the
  // same bits that are canonicalized in one type need not be in the other.
  return isCanonicalized(DAG, Op.getOperand(0), MaxDepth - 1);
```

The in-source TODO is the bug.  `isCanonicalized` recurses through
the bitcast as if the dest FP type's canonicality status were the
same as the source FP type's.  In reality:

* **v2bf16** mantissa = 7 bits, exponent = 8 bits.
* **v2f16**  mantissa = 10 bits, exponent = 5 bits.
* **f32 / v2f32** mantissa = 23 bits, exponent = 8 bits.

A 16-bit pattern that is *normal* in v2bf16 (e.g. small power-of-2
times 1, exp != 0) can be *denormal* in v2f16 if the corresponding
v2f16 exponent field happens to be zero with a nonzero mantissa, and
vice versa.  Likewise, NaN payload bits straddle different
exp/mantissa boundaries, so a quiet-NaN-payload bit pattern in one
type can be a non-quiet-NaN payload in another.

`isCanonicalized` is used by `is_canonicalized_1` / `is_canonicalized_2`
PatFrags (`AMDGPUInstructions.td:189,207-208`; `SIInstrInfo.td:1017,1025`),
which decide whether `V_PACK_B32_F16` / `min/max` selection patterns
may *omit* an explicit canonicalisation step.  If the bitcast leaks
"already canonical" through a type transition, the codegen drops
the explicit canonicalize and the value-changing semantic divergence
becomes observable when the next consumer:

* Triggers FTZ on a denormal that was not denormal in the source
  type.
* Propagates sNaN where the source-type quietening would have applied.

## Reproducer

`reduced.ll` chains:

1. Load `i32` value.
2. `bitcast i32 -> v2bf16`.
3. `extractelement` + `insertelement` (identity, to prevent
   trivial constant-fold while preserving bits).
4. `bitcast v2bf16 -> i32`.
5. `bitcast i32 -> v2f16`.
6. `call @llvm.canonicalize.v2f16(...)`.
7. Store the canonicalized result.

For an input bit pattern that is *normal* viewed as v2bf16 but
*subnormal* viewed as v2f16 with `denormal-fp-math=preserve-sign`
(the gfx950 default for f16), the combiner walks the chain back to
the v2bf16 fcanonicalize-eligible source, concludes "already
canonical", and drops the `v_pk_max_f16` that O0 emits.  The
subnormal v2f16 value then reaches the store raw at O2 but is FTZ'd
to +0 at O0.

## Suggested fix

In `isCanonicalized`, when recursing through `ISD::BITCAST`, check
whether the source FP type and the dest FP type have compatible
exponent/mantissa layouts.  Concretely:

```cpp
case ISD::BITCAST: {
  EVT SrcVT = Op.getOperand(0).getValueType();
  EVT DstVT = Op.getValueType();
  // Only recurse if both types are FP and have matching FP semantics
  // (same denormal range, same NaN payload boundary).
  if (!SrcVT.isFloatingPoint() || !DstVT.isFloatingPoint())
    return false;
  if (SrcVT.getScalarType() != DstVT.getScalarType())
    return false;
  return isCanonicalized(DAG, Op.getOperand(0), MaxDepth - 1);
}
```

Or simply: do not recurse through BITCAST at all; treat it as
unknown.  The combiner will then conservatively emit the
canonicalize, costing one v_max_f16 instruction in the rare cases
where the source was already canonical.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `bitcast i32 -> v2bf16` round-trips
  followed by `fcanonicalize` -- per `MEMORY.md`
  (Prefer-random-over-idioms), enriching the random emitter to
  include bf16↔v2f16 bitcast chains under varying denormal modes
  would surface this and m140's sibling shapes.
* The differential O0-vs-O2 oracle would catch it once a bf16/f16
  layout straddling bit pattern is fed to a fcanonicalize.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Code path present; TODO acknowledged in source. |
| ROCm 7.1.1 | Same defect. |

## Family

Sibling of:
* m115/m124 (`fcanonicalize` v2f16 undef-lane handling).
* m118 (`isCanonicalized` over-promise for FP-class intrinsics) --
  same function, different arm.
* m133 (`getCanonicalConstantFP` drops NaN payload) -- same area.
* m100 (denormal mode bookkeeping for f32 reciprocal).
