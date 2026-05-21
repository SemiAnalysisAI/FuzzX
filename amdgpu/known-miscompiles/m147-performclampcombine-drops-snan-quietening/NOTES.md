# m147: `performClampCombine` constant-fold returns sNaN unchanged; HW `v_max_f32(sNaN, sNaN)` quiets it

*Discovery method: code inspection (during getCanonicalConstantFP
adjacent constant-fold audit).*  Sibling shape to m133 (same defect
class, different opcode and code path).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:18284-18303`
(`performClampCombine`):

```cpp
auto Bound = ...;
if (F < Bound || (F.isNaN() && Subtarget->enableDX10Clamp()))
  return DAG.getConstantFP(Bound, SL, VT);
if (F > Top)
  return DAG.getConstantFP(Top, SL, VT);
return SDValue(CSrc, 0);              // <-- sNaN passes through here
```

`AMDGPUISD::CLAMP` lowers via `ClampPat` (`SIInstructions.td:2030-2036`)
to:

```asm
V_MAX_F32_e64 src, src, DSTCLAMP.ENABLE
```

With `IEEE_MODE=1` (default for compute on gfx950) and
`DX10Clamp=OFF` (default for non-graphics kernels),
`v_max_f32(sNaN, sNaN)` **quiets the sNaN** (sets mantissa bit 22)
but preserves the payload tail.

The constant-fold returns the sNaN bit-pattern unchanged.  The HW
lowering would produce the corresponding qNaN.  Difference is
observable when the result is stored as i32 / bitcast.

For input `0x7F800001` (sNaN with payload 1):

| Path | Result |
| --- | --- |
| Constant-fold (`performClampCombine`) | `0x7F800001` (raw sNaN, payload 1) |
| HW (`V_MAX_F32_e64`) | `0x7FC00001` (quieted, bit 22 set, payload 1 preserved) |

## Reproducer

`reduced.ll` constructs the CLAMP via `amdgcn.fmed3.f32(c, 0.0, 1.0)`
where `c` is a constant sNaN.  `performFPMed3ImmCombine`
(`SIISelLowering.cpp:16057`) recognises the pattern and emits
`AMDGPUISD::CLAMP(c)`, which `performClampCombine` then constant-folds.

```llvm
%s = call float @llvm.amdgcn.fmed3.f32(
    float bitcast (i32 2139095041 to float),    ; sNaN payload=1
    float 0.0,
    float 1.0)
store float %s, ptr addrspace(1) %p
```

Expected (HW): `0x7FC00001`.  Observed (constant-folded at O2):
`0x7F800001`.

## Trigger reachability

`AMDGPUISD::CLAMP` is created by:

* `performFPMed3ImmCombine` (line 16057): `fmed3(c, 0.0, 1.0)` ->
  CLAMP(c).
* TableGen pattern matchers at lines 16280, 16301:
  `fminnum(fmaxnum(c, 0.0), 1.0)` -> CLAMP(c).
* Direct user via `llvm.amdgcn.fmed3.f32` with constants {0.0, 1.0}.

The pattern-matcher path requires no fast-math flags or special
attributes -- any IR shape that reduces to a clamp of an sNaN
constant triggers the bug.

## Suggested fix

Mirror the m133 fix shape: for sNaN, return the quieted version
with payload preserved (set precision-2 bit / mantissa bit 22 for
f32):

```cpp
if (F.isSignaling()) {
  APInt Q = F.bitcastToAPInt();
  Q.setBit(F.getSemantics().precision - 2);
  return DAG.getConstantFP(APFloat(F.getSemantics(), Q), SL, VT);
}
```

This matches the HW `v_max_f32` quietening behavior.  The denormal
case (Bound <= F <= Top) is correctly left unchanged because
`ClampPat`'s in-source comment at `SIInstructions.td:2028-2029`
confirms "the output result is not flushed" for `v_max_f32`-as-clamp.

## Why the fuzzer hasn't caught it

* Default oracle compares scalar/value outputs O0 vs O2 with `bnan`
  comparison that may mask sNaN payload bit differences.  Per
  `MEMORY.md` (Prefer-random-over-idioms), the oracle should be
  augmented to compare NaN bit patterns exactly (or at least the
  sign + quiet-bit + top-9 payload bits).
* The random emitter should bias toward `fmed3(c, 0.0, 1.0)` and
  `fminnum(fmaxnum(c, 0.0), 1.0)` shapes with sNaN constants in `c`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Constant-fold returns raw sNaN. |
| ROCm 7.1.1 | Same defect. |

## Family

* m133 (`getCanonicalConstantFP` drops NaN payload while HW
  preserves) -- same defect class.
* m137 (`LowerF64ToF16Safe` drops NaN payload).
* m115/m124/m118/m141 (broader fcanonicalize / canonical-property
  family).
