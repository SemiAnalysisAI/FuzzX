## ConvertNodeToLibcall: STRICT_F{ADD,SUB,MUL,DIV,SQRT} can be routed to FAST_* libcall, violating strict-FP exception/rounding contract

`llvm/lib/CodeGen/SelectionDAG/LegalizeDAG.cpp:4855-4866, 5121-5161, 5334-5343`
helper at `LegalizeDAG.cpp:4727-4734` (`canUseFastMathLibcall`)
helper at `LegalizeDAG.cpp:2259-2288` (`ExpandFastFPLibCall`)

```cpp
static bool canUseFastMathLibcall(const SDNode *Node) {
  // FIXME: Probably should define fast to respect nan/inf and only be
  // approximate functions.
  SDNodeFlags Flags = Node->getFlags();
  return Flags.hasApproximateFuncs() && Flags.hasNoNaNs() &&
         Flags.hasNoInfs() && Flags.hasNoSignedZeros();
}

case ISD::FSQRT:
case ISD::STRICT_FSQRT: {
  ExpandFastFPLibCall(Node, canUseFastMathLibcall(Node),
                      {RTLIB::FAST_SQRT_F32, RTLIB::SQRT_F32},
                      ...
}
case ISD::FADD:
case ISD::STRICT_FADD: { ExpandFastFPLibCall(...FAST_ADD_..., ADD_...); }
case ISD::FMUL:
case ISD::STRICT_FMUL: { ExpandFastFPLibCall(...FAST_MUL_..., MUL_...); }
case ISD::FDIV:
case ISD::STRICT_FDIV: { ExpandFastFPLibCall(...FAST_DIV_..., DIV_...); }
case ISD::FSUB:
case ISD::STRICT_FSUB: { ExpandFastFPLibCall(...FAST_SUB_..., SUB_...); }
```

`canUseFastMathLibcall` inspects only the SDNode's fast-math flags and never
checks `Node->isStrictFPOpcode()`. Constrained intrinsics in LLVM IR are allowed
to carry FMF (`call double @llvm.experimental.constrained.fadd.f64(... "afn nnan
ninf nsz")`), and SelectionDAGBuilder propagates those flags to the
`STRICT_F*` SDNode. When `ConvertNodeToLibcall` then processes a
`STRICT_FADD/STRICT_FMUL/STRICT_FDIV/STRICT_FSUB/STRICT_FSQRT` with all four
flags set, `canUseFastMathLibcall` returns true and `ExpandFastFPLibCall` picks
the `RTLIB::FAST_*` libcall.

The "fast" libcalls (Hexagon's `__hexagon_fast2_sqrtf`, etc., in
`RuntimeLibcalls.td:118-134, 2771-2789`) are approximation routines whose entire
purpose is to bypass IEEE‑754 conformance: they do not raise FE exceptions
correctly, do not honor non-default rounding modes, and produce results that
differ from the strict-FP contract advertised by the constrained intrinsic
(`fpexcept.strict` / `round.tonearest` etc.). `ExpandFPLibCall`'s strict path
(`LegalizeDAG.cpp:2226-2237`) faithfully passes the chain through `makeLibCall`,
which means the FAST libcall is emitted as a real call sequence — but the
callee is the wrong function for the IR-level semantics.

Today x86 does not register `FAST_*` libcall implementations (only Hexagon does
in `RuntimeLibcalls.td`), so the fallback at `LegalizeDAG.cpp:2280` kicks in
and the STRICT op gets the IEEE libcall. The bug is latent for the x86 target
but is a structural defect: any target that registers a `FAST_*` impl in the
future, or any backend that re-uses this generic helper for cross‑compilation
to Hexagon, will silently miscompile STRICT constrained intrinsics that happen
to carry FMF.

### Why FMF on a STRICT_F* node is reachable

SelectionDAGBuilder.cpp's `visitConstrainedFPIntrinsic` copies the call's FMF
onto the STRICT SDNode via `Flags.copyFMF(*FPI)`. Optimization passes
(InstCombine, GVN's PRE of math intrinsics, LoopVectorize partial-reduction
rewrite) preserve or insert FMF on constrained intrinsics derived from
fast-math sources. A `strictfp` function whose user wrote
`__builtin_sqrtf(x)` under `-ffast-math -ffp-exception-behavior=strict`
produces exactly this shape.

### Fix sketch

```cpp
static bool canUseFastMathLibcall(const SDNode *Node) {
  if (Node->isStrictFPOpcode())
    return false;     // strict-FP requires IEEE-conformant libcall
  SDNodeFlags Flags = Node->getFlags();
  return Flags.hasApproximateFuncs() && Flags.hasNoNaNs() &&
         Flags.hasNoInfs() && Flags.hasNoSignedZeros();
}
```

### Candidate IR (latent on x86; reproducer needs a target with FAST_* impl)

```
declare float @llvm.experimental.constrained.fadd.f32(float, float, metadata, metadata)

define float @latent_fast_add(float %x, float %y) #0 {
  %r = call afn nnan ninf nsz float
        @llvm.experimental.constrained.fadd.f32(
          float %x, float %y,
          metadata !"round.tonearest",
          metadata !"fpexcept.strict")
  ret float %r
}
attributes #0 = { strictfp }
```

Build/run `llc -mtriple=hexagon-unknown-elf -mattr=+hvxv60 -fp-contract=fast`
(needs Hexagon llc) and observe `__hexagon_fast_addsf3` emitted for a node
that promised `fpexcept.strict`. On x86 the same IR routes to `__addsf3`
(no `FAST_ADD_F32` impl), so the visible miscompile would require either
a new FAST_* registration or a custom-target follow-on.

### Severity

Latent. Filed because the gating decision (`canUseFastMathLibcall`) ignores a
property (strictness) that is the entire reason the user picked the constrained
intrinsic. This is the kind of structural mismatch the fuzzer would hit the
moment any backend adds FAST_* libcall impls, and it should be guarded at the
source.
