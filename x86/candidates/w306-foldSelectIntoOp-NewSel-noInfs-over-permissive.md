# w306: foldSelectIntoOp sets `noInfs` on NewSel from TVI without verifying NewSel arms are finite

## Bug

`InstCombinerImpl::foldSelectIntoOp` in
`llvm/lib/Transforms/InstCombine/InstCombineSelect.cpp` lines
569-658, builds an intermediate select:

```cpp
Value *NewSel = Builder.CreateSelect(SI.getCondition(),
                                     Swapped ? C : OOp,
                                     Swapped ? OOp : C, "", &SI);
if (isa<FPMathOperator>(&SI)) {
  FastMathFlags NewSelFMF = FMF;                       // outer Sel FMF
  ...
  NewSelFMF.setNoInfs(TVI->hasNoInfs() ||
                      (CanInferFiniteOperandsFromResult &&
                       NewSelFMF.noInfs() && NewSelFMF.noNaNs()));
  cast<Instruction>(NewSel)->setFastMathFlags(NewSelFMF);
}
```

(lines 615-635)

The condition `TVI->hasNoInfs()` is propagated onto `NewSel`. But
`NewSel = select(Cond, OOp, IdC)` — OOp could itself be `+inf`/`-inf`
even when `TVI = X op OOp` is non-infinity (e.g. `fmul 0.0, +inf =
NaN`, not infinity → satisfies `noInfs` on TVI).

If `OOp` is `+inf` and `Cond=true`, then `NewSel = +inf`. But NewSel
has the `noInfs` flag set, which per LangRef means "the value will
not be inf — violating this gives poison". So NewSel is poison.

Then `BO = TVI.opcode(FalseVal, NewSel)` — with NewSel=poison, BO is
poison.

Original `SI` with `Cond=true` returns `TVI = X * OOp = 0 * inf = NaN`.
That's a defined value (NaN, not poison).

This is a refinement (poison ≤ value), so it's permitted. But it's
worth checking whether any other downstream pass can incorrectly
consume the propagated `noInfs` flag to make further deductions about
NewSel that wouldn't be valid for the original select.

The risk is highest if a later optimization re-derives properties
about OOp from NewSel's `noInfs` flag (e.g. a value-tracking pass
inferring that OOp != inf when Cond=true).

## Repro (illustrative)

```llvm
; opt -passes=instcombine -S w306.ll
define float @sel_fmul(i1 %c, float %x, float %y) {
  %m = fmul ninf float %x, %y
  %s = select i1 %c, float %m, float %x
  ret float %s
}
```

The TryFoldSelectIntoOp path triggers when `FalseVal == TVI->getOperand(0)`
(here `x = TVI->getOperand(0)` for the `m_c_BinOp` commutative match).

Per check on lines 597-602, OOp constant constraint may block the
trivial case here; need OOp variable.

## Source cite

- `llvm/lib/Transforms/InstCombine/InstCombineSelect.cpp:569-658`
  (foldSelectIntoOp full function)
- `llvm/lib/Transforms/InstCombine/InstCombineSelect.cpp:617-634`
  (NewSel FMF assignment — the suspicious bit is line 631-633)

## Severity

Refinement-permitted but potentially exposing latent bug in
downstream passes. Likely missed-opt rather than miscompile.

## Fix sketch

When propagating `noInfs` to NewSel, restrict to cases where the
arms (OOp and IdC) are known non-infinity. The conservative fix:

```cpp
NewSelFMF.setNoInfs(NewSelFMF.noInfs() &&
                    cannotBeInfinity(OOp, SQ.getWithInstruction(&SI)));
```

The current code is OPTIMISTIC and only safe if NewSel poison is
acceptable, which it is by refinement but undermines `noInfs`'s
contract.
