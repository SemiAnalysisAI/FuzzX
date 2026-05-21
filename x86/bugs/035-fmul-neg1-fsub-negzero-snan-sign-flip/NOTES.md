# w12: `fmul X, -1.0` -> `fsub -0.0, X` flips NaN-sign without nnan

**File:lines:** `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:19266-19272` (visitFMUL)

## Reasoning

```cpp
// fold (fmul X, -1.0) -> (fsub -0.0, X)
if (N1CFP && N1CFP->isExactlyValue(-1.0)) {
  if (!LegalOperations || TLI.isOperationLegal(ISD::FSUB, VT)) {
    return DAG.getNode(ISD::FSUB, DL, VT,
                       DAG.getConstantFP(-0.0, DL, VT), N0, Flags);
  }
}
```

No fast-math flag is required. Mathematically `(-1.0) * X == 0 - X`, but for
NaN operands the IR LangRef gives `fmul` permission to choose any NaN payload,
while `fsub -0.0, X` must propagate X's NaN. **Sign-bit behavior differs**: per
LLVM's IR semantics, `fmul X, -1.0` does not specify that X's sign bit is
flipped (the NaN payload/sign is unspecified). `fsub -0.0, NaN` is also free to
choose, but in practice the combination of this fold with the FSUB->FNEG fold
above means `fmul X, -1.0` ends up lowering to `xor X, sign_mask` — i.e. the
sign of an input NaN is flipped, even though the user's source `fmul X, -1.0`
did not request that.

Combined with the FSUB FIXME, this also drops sNaN-quieting that the original
`fmul` IR may have provided.

## Candidate IR / x86 codegen

```ll
define float @f(float %x) {
  %r = fmul float %x, -1.0
  ret float %r
}
```

`llc -mtriple=x86_64` produces:
```
f:
  xorps .LCPI0_0(%rip), %xmm0     ; 0x80000000 mask -- bare FNEG
  retq
```

The chain is: visitFMUL rewrites `fmul X, -1.0` -> `fsub -0.0, X`. visitFSUB
then rewrites that to `fneg X`. Final asm is a sign-bit toggle, with no NaN
quieting.

## Expected wrong outcome

Pass sNaN `0x7FA00000`. The generated code returns `0xFFA00000` (sNaN,
sign-flipped). The original `fmul X, -1.0` per IR semantics may produce any NaN
payload — but most users expect at least that a signaling NaN is quieted to a
qNaN when it goes through an arithmetic op. Pipelined with the FSUB FIXME, this
gives users an observable sNaN -> sNaN passthrough they didn't write.

(Lower severity than the minimumnum bug above since IR `fmul` allows NaN
payload choice — but worth flagging as the surface where the FSUB FIXME bites
even when the user wrote `fmul`, not `fsub`.)
