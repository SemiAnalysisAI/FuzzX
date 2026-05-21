# m139: `performFNegCombine` FMA arm: `(fneg (fma x, y, z)) -> (fma x, -y, -z)` flips NaN sign

*Discovery method: random IR fuzzing (v2f16 NaN-seeded chains),
empirically verified on gfx950 HW.*

Sibling shape to the m107 / m110 / m111 / m120 / m127 family in the
same combine function -- this is the FMA arm (m107 covered the FMUL
arm).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:5319-5347`
(`AMDGPUTargetLowering::performFNegCombine`, FMA / FMAD case):

```cpp
case ISD::FMA:
case ISD::FMAD: {
  if (!mayIgnoreSignedZero(N0))
    return SDValue();

  // (fneg (fma x, y, z)) -> (fma x, (fneg y), (fneg z))
  SDValue Y = N0.getOperand(1);
  SDValue Z = N0.getOperand(2);
  ...
  SDValue NegY = DAG.getNode(ISD::FNEG, SDLoc(N), VT, Y, N0->getFlags());
  SDValue NegZ = DAG.getNode(ISD::FNEG, SDLoc(N), VT, Z, N0->getFlags());
  return DAG.getNode(ISD::FMA, SDLoc(N), VT, X, NegY, NegZ, N0->getFlags());
}
```

The combine is gated only on `nsz` (`mayIgnoreSignedZero`).  But for
NaN inputs `fma(x, -y, -z)` selects a NaN payload/sign from a
different operand chain than `-fma(x, y, z)` would.

`fneg` on a NaN flips exactly the sign bit (LangRef / IEEE-754).
`fma(...)` with at least one NaN operand returns a NaN whose payload
is implementation-defined and may come from any of the three NaN
operands.  Substituting `-y, -z` for `y, z` changes which operand is
NaN (or which NaN's sign is observed first) and therefore changes
the result NaN's sign bit.

`nsz` is the wrong gate.  The combine needs `nnan` (or a guard that
none of x/y/z can be NaN).

## Reproducer

`reduced.ll` (v2f16):

```llvm
%v0 = call contract nsz <2 x half> @llvm.fma.v2f16(<2 x half> %in0,
                                                   <2 x half> %in2,
                                                   <2 x half> %in0)
%v6 = fsub contract <2 x half> <half -0.0, half -0.0>, %v0   ; = fneg(v0)
store i32 (bitcast v6 to i32), ptr addrspace(1) %out
```

Inputs:
* `in0 = 0xfe00fc00` (top half: -qNaN payload 0; bottom half: -Inf)
* `in2 = 0x7c007c00` (both halves: +Inf)

So `v0 = fma(in0, in2, in0)`:
* top half: `fma(-qNaN, +Inf, -qNaN)` -> NaN (HW returns -qNaN propagated).
* bottom half: `fma(-Inf, +Inf, -Inf)` -> -Inf * +Inf + -Inf = -Inf - Inf = -Inf.

Then `v6 = fneg(v0)`:
* top half: NaN with flipped sign = +qNaN.
* bottom half: +Inf.

Expected top-half: `0x7c00 | sign-of-flipped-NaN = 0x7c00` (positive qNaN payload 0).

```
=== -O0 ===
v_pk_fma_f16 v1, v1, v2, v1
v_xor s2, 0x80008000, v1                ; explicit fneg via xor
-> stores 0x7e007c00                    ; top half NaN with sign flipped per fneg semantics

=== -O2 ===
v_pk_fma_f16 v1, v1, v2, v1 neg_lo:[0,1,1] neg_hi:[0,1,1]
                                         ; FMA arm fired: NegY=fneg(v2), NegZ=fneg(v0)
-> stores 0xfe007c00                    ; top half NaN sign NOT flipped (kept negative)
```

The same IR yields different NaN sign at top half between O0 and O2.
LangRef requires `fneg` to flip the sign bit precisely; the
substitution `-fma(x,y,z) -> fma(x,-y,-z)` does NOT preserve that
guarantee for NaN inputs.

## Suggested fix

Replace the `nsz`-only gate with `nsz && nnan`, or fold the
negate-into-input-modifier only when proven the FMA cannot return a
NaN whose sign bit is observable.  Concretely:

```cpp
case ISD::FMA:
case ISD::FMAD: {
  if (!mayIgnoreSignedZero(N0))
    return SDValue();
  if (!N0->getFlags().hasNoNaNs())     // <-- ADD
    return SDValue();
  ...
}
```

The same `nnan`-missing pattern shows up in adjacent arms; per
`MEMORY.md` (Prefer-random-over-idioms), an audit of all
`mayIgnoreSignedZero`-only gated rewrites in `performFNegCombine`
should be folded into the same fix.  m107 (FMUL), m120 (FMul
fneg-LHS), m127 (FSub fadd folds), and m128 (FDOT2) are all in this
family.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`O0=0x7e007c00, O2=0xfe007c00`). |
| ROCm 7.1.1 | Reproduces. |

Same defect across all 3 campaign toolchains.

## Why the fuzzer hasn't caught it

* Default FuzzX scalar emitter rarely produces NaN-seeded FMA chains
  whose output is observed via `fneg`.  Per `MEMORY.md`
  (Prefer-random-over-idioms), enriching the random FP emitter to
  bias toward NaN-input chains with terminal `fneg` would surface
  this entire arm family.
* The aux v2f16 fuzz harness (NaN-biased constant pool, mixed
  intrinsic chains) caught this on its 20th seed.

## Distinguishing v2f16_17, _19, _509 (HW artifact, NOT filed)

Three other fuzz seeds in the same campaign showed apparent
v2f16-fmul NaN-payload divergence between O0 and O2, but verification
showed they reduce to register-allocation order differences in the
`v_pk_mul_f16` src0/src1 encoding.  AMDGPU HW `v_pk_mul_f16` selects
the result NaN payload based on the physical src0 vs src1 slot, so
two encoded variants of the same logical mul can return different
NaN payloads.  Not a compiler combine bug, not filed.
