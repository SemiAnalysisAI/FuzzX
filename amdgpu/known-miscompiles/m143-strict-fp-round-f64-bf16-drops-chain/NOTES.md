# m143: `STRICT_FP_ROUND f64 -> bf16` silently drops the strict chain and FP exception semantics

*Discovery method: code inspection (during STRICT_FP_* combiner / lowering audit).*

Sibling shape to m137 (`LowerF64ToF16Safe` NaN payload divergence) and
m118 (bf16 over-promise) -- AMDGPU strict-FP lowering for bf16 is not
audited.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:8604-8613`
(`SITargetLowering::lowerFP_ROUND`, f64 -> bf16 path):

```cpp
assert(DstVT.getScalarType() == MVT::bf16);
SDValue Round = TargetLowering::expandRoundInexactToOdd(
    F32VT, Src, DL, DAG);
return DAG.getNode(ISD::FP_ROUND, DL, DstVT, Round,
                   DAG.getIntPtrConstant(0, DL,
                                         /*isTarget=*/true));
```

The only strict-FP guard in this function is on the **f64 -> f16
path** (lines 8585-8587):

```cpp
// TODO: Handle strictfp
if (Op->isStrictFPOpcode())
  return Op;
```

The f64 -> bf16 path falls through that guard without checking for
the strict opcode.  For `ISD::STRICT_FP_ROUND` with src f64 and dst
bf16 the lowering then:

1. Calls `TargetLowering::expandRoundInexactToOdd`
   (`TargetLowering.cpp:12840`) which builds a non-strict graph:
   `getFPExtendOrRound`, `FABS`, `setcc(SETUEQ/SETOGT)`, arithmetic.
   None of these nodes carry the strict chain or raise FP exceptions.

2. Emits a non-strict `ISD::FP_ROUND` at line 8612.

Result:

* The strict node's chain (operand 0) is never threaded through; the
  second result (chain) of `STRICT_FP_ROUND` is silently lost.
* Exception semantics from the double-rounding-to-odd intermediate
  computation and the final round are dropped.
* Downstream nodes that should be ordered after the strict round can
  reorder arbitrarily.

## Reproducer

`reduced.ll`:

```llvm
declare bfloat @llvm.experimental.constrained.fptrunc.bf16.f64(double, metadata, metadata)

define amdgpu_kernel void @t(ptr addrspace(1) %p, double %x) #0 {
  %r = call bfloat @llvm.experimental.constrained.fptrunc.bf16.f64(
         double %x,
         metadata !"round.tonearest",
         metadata !"fpexcept.strict") #0
  store bfloat %r, ptr addrspace(1) %p, align 2
  ret void
}

attributes #0 = { strictfp }
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll`: the emitted code
omits any sentinel that the strict node's chain ordered downstream
operations, and the FP-exception raising of the inexact-to-odd
expansion is silently elided.

## Suggested fix

In `SITargetLowering::lowerFP_ROUND`, hoist the strict-FP short-circuit
above the bf16 branch:

```cpp
// TODO: Handle strictfp
if (Op->isStrictFPOpcode())
  return SDValue();    // bail; let the legalizer expand
```

A more thorough fix wires a real strict expansion: `STRICT_FP_EXTEND`
(if any) + strict arithmetic on the intermediate f32 +
`STRICT_FP_ROUND` to bf16.  The current `expandRoundInexactToOdd`
helper has no strict variant; either add one or scalarise.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `llvm.experimental.constrained.fptrunc`
  with `bf16` destination type.  Per `MEMORY.md`
  (Prefer-random-over-idioms), the random emitter should add
  `constrained.fptrunc.bf16.{f32,f64}` to the intrinsic pool.
* The O0-vs-O2 differential oracle would catch chain reordering only
  if a side-effecting op (e.g. a store) is positioned to move past
  the strict round.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Code path present; strict chain dropped. |
| ROCm 7.1.1 | Same defect. |

## Adjacent defects in same function

Two siblings flagged in the same audit:

* `lowerFP_ROUND` f64 -> f16 strict path (line 8585) returns
  `Op` unchanged with "TODO" comment -- defers to default expansion
  which may also be unsound.
* `lowerFP_EXTEND` bf16 -> f32/f64 strict path (`SIISelLowering.cpp:4914-4915`)
  hits `llvm_unreachable("Need STRICT_BF16_TO_FP")` -- compiler crash,
  filed as **c010**.
