# m124: `performFCanonicalizeCombine` v2f16 BUILD_VECTOR path decays `fcanonicalize(<2 x half> undef)` to `<0.0, 0.0>` instead of `<qNaN, qNaN>`

*Discovery method: code inspection.*  Distinct from m115 (lane-0 undef
+ runtime lane-1) -- this bug fires when BOTH lanes are undef.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:15861-15928`
(`performFCanonicalizeCombine`).

LangRef `llvm.canonicalize` on `undef` should produce a quiet NaN
(the canonical value).  The combine has a correct scalar undef arm
at line 15868:

```cpp
if (N->getOperand(0).isUndef()) {
  EVT VT = N->getValueType(0);
  EVT EltVT = VT.getScalarType();
  return DAG.getConstantFP(
      APFloat::getQNaN(EltVT.getFltSemantics()), SDLoc(N), VT);
}
```

But SDAG lowers `<2 x half> undef` as `BUILD_VECTOR undef, undef`,
NOT as a single undef SDValue.  So `N->getOperand(0).isUndef()`
returns false and the combine falls into the v2f16 BUILD_VECTOR
path at 15885+ instead:

1. Lane 1 fixup at lines 15917-15921 sees both NewElts undef and
   correctly falls back: `NewElts[1] = DAG.getConstantFP(0.0f, SL, EltVT)`.
2. Lane 0 fixup at lines 15910-15915 (the buggy ternary documented
   by m115) then sees `NewElts[1]` is a `ConstantFPSDNode`, so it
   splats: `NewElts[0] = NewElts[1] = 0.0`.

Result: `<0.0, 0.0>` (packed `0x00000000`) instead of
`<qNaN, qNaN>` (packed `0x7E007E00`).

The corresponding scalar undef arm correctly returns qNaN.  The v4f16
path also works because type-legalization splits it to two scalar
undef arms.  Only the v2f16-specific BUILD_VECTOR path is wrong.

## Reproducer

`reduced.ll`:

```llvm
declare <2 x half> @llvm.canonicalize.v2f16(<2 x half>)

define amdgpu_kernel void @t(ptr addrspace(1) %out) {
  %c = call <2 x half> @llvm.canonicalize.v2f16(<2 x half> undef)
  %bc = bitcast <2 x half> %c to i32
  store i32 %bc, ptr addrspace(1) %out
  ret void
}
```

Codegen with `clang -mcpu=gfx950 -O2`:

```asm
v_mov_b32_e32 v0, 0
global_store_dword v0, v0, s[0:1]
```

Stores `0x00000000` (two `+0.0` half values) instead of the expected
`0x7E007E00` (two qNaN halves).

Compare with v4f16 (`t_v4` in `reduced.ll`):

```llvm
%c = call <4 x half> @llvm.canonicalize.v4f16(<4 x half> undef)
```

This correctly stores `0x7E007E007E007E00` because type-legalization
splits the v4f16 into two scalar canonicalize calls, each of which
hits the correct scalar undef arm.  The asymmetry between v2f16
(buggy) and v4f16 (correct) is the cleanest evidence that this is a
v2f16-BUILD_VECTOR-path bug.

## Suggested fix

Add an early check at the top of the v2f16 BUILD_VECTOR path (around
line 15885) for the all-undef case:

```cpp
if (Src.getOpcode() == ISD::BUILD_VECTOR &&
    VT.getVectorElementType() == MVT::f16 &&
    VT.getVectorNumElements() == 2) {
  // Reject all-undef early -- fall through to the qNaN arm above.
  if (llvm::all_of(Src->op_values(),
                   [](SDValue V) { return V.isUndef(); }))
    return DAG.getConstantFP(
        APFloat::getQNaN(EltVT.getFltSemantics()), SDLoc(N), VT);
  ...
}
```

Or fix the dead ternary at 15910-15915 (per m115) so that when both
lanes are undef, lane 0 also falls back to qNaN.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`v_mov_b32 v0, 0`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same combine, same bug. |

## Why the fuzzer hasn't caught it

* The FP emitter rarely emits `<2 x half> undef` directly as the
  operand of `llvm.canonicalize`.
* The interpreter oracle treats undef as 0.0 -- which happens to
  match the buggy SDAG output, masking the bug.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  emit `llvm.canonicalize.v2f16(<2 x half> undef)` patterns in the
  random emitter and use a stricter undef-to-qNaN oracle.
