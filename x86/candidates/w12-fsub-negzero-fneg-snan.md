# w12: `fsub -0.0, X` -> `fneg X` drops sNaN-quieting (known FIXME)

**File:lines:** `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:19053-19068` (visitFSUB)

## Reasoning

`visitFSUB` has the transform:

```cpp
// (fsub -0.0, N1) -> -N1
if (N0CFP && N0CFP->isZero()) {
  if (N0CFP->isNegative() || DAG.canIgnoreSignBitOfZero(SDValue(N, 0))) {
    // FIXME: This transform will change the sign of a NaN and the behavior
    // of a signaling NaN. It is only valid when a NoNaN flag is present.
    DenormalMode DenormMode = DAG.getDenormalMode(VT);
    if (DenormMode == DenormalMode::getIEEE()) {
      if (SDValue NegN1 = TLI.getNegatedExpression(...))
        return NegN1;
      if (!LegalOperations || TLI.isOperationLegal(ISD::FNEG, VT))
        return DAG.getNode(ISD::FNEG, DL, VT, N1);
    }
  }
}
```

The inline FIXME confirms the issue: per IR LangRef, `fsub` quiets a signaling
NaN operand; `fneg` only flips the sign bit. The transform unconditionally
rewrites `fsub -0.0, X` to `FNEG X` whenever the denormal mode is IEEE, with no
nnan check. On x86, FNEG lowers to an xor with the sign-bit mask, which
preserves the signaling bit pattern. The FSUB form would lower to a real subss
that quiets sNaN.

## Candidate IR

```ll
define float @f(float %x) {
  %r = fsub float -0.0, %x        ; no fast-math flags
  ret float %r
}
```

`llc -mtriple=x86_64` produces:
```
f:
  xorps .LCPI0_0(%rip), %xmm0     ; .long 0x80000000 (sign mask) -- just FNEG
  retq
```

## Expected wrong outcome

Call with sNaN `0x7FA00000`. Correct IR semantics for `fsub -0.0, sNaN`: result
should be a qNaN (quieting bit set, `0xFFE00000` after sign flip — or any qNaN
payload). The generated xorps just flips the sign, yielding `0xFFA00000` —
still a signaling NaN. On targets/contexts that observe the sNaN-vs-qNaN
distinction (e.g. enabled invalid-op trapping via MXCSR, or downstream code that
inspects bit patterns), this is a miscompile.

The FIXME in source acknowledges this; flagging as the known semantic gap that
worker-12 should report for x86 codegen.
