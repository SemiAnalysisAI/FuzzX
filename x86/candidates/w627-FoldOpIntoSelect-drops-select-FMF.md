# w627: FoldOpIntoSelect drops select's FMF when folding a cast into a select

## Source
- File: `llvm/lib/Transforms/InstCombine/InstructionCombining.cpp`
- Function: `InstCombinerImpl::FoldOpIntoSelect`
- Lines: 1834 (final SelectInst::Create), 1773-1778 (foldOperationIntoSelectOperand)

## Code

```cpp
Instruction *InstCombinerImpl::FoldOpIntoSelect(Instruction &Op, SelectInst *SI,
                                                bool FoldWithMultiUse,
                                                bool SimplifyBothArms) {
  ...
  // Create an instruction for the arm that did not fold.
  if (!NewTV)
    NewTV = foldOperationIntoSelectOperand(Op, SI, TV, *this);
  if (!NewFV)
    NewFV = foldOperationIntoSelectOperand(Op, SI, FV, *this);
  return SelectInst::Create(SI->getCondition(), NewTV, NewFV, "", nullptr, SI);
  //                                                              ^^^^^^^^^^^^
  //                                                       MDFrom only copies metadata,
  //                                                       not FastMathFlags.
}
```

`SelectInst::Create(C, S1, S2, Name, InsertBefore, MDFrom=SI)` calls
`Sel->copyMetadata(*MDFrom)` (`llvm/include/llvm/IR/Instructions.h:1739-1740`).
`copyMetadata` copies metadata (debug locs etc.), but `FastMathFlags` are NOT
metadata — they live on the instruction itself. So the new select drops the
original select's FMF (e.g. `nnan`, `ninf`, `nsz`, `fast`, ...).

## Repro: `/tmp/icc625/fpt-sel.ll`

```llvm
target triple = "x86_64-unknown-linux-gnu"

define float @fpt_sel_ninf(i1 %c, float %x, double %y) {
  %xe = fpext float %x to double
  %s = select nsz i1 %c, double %xe, double %y    ; <-- 'nsz' here
  %r = fptrunc ninf double %s to float            ; <-- 'ninf' here
  ret float %r
}

define float @fpt_sel_sel_fast(i1 %c, float %x, double %y) {
  %xe = fpext float %x to double
  %s = select fast i1 %c, double %xe, double %y   ; <-- 'fast' here
  %r = fptrunc double %s to float
  ret float %r
}
```

`opt -passes=instcombine -S`:

```llvm
define float @fpt_sel_ninf(i1 %c, float %x, double %y) {
  %1 = fptrunc ninf double %y to float     ; ninf preserved (instruction was cloned)
  %r = select i1 %c, float %x, float %1    ; <-- 'nsz' LOST
  ret float %r
}

define float @fpt_sel_sel_fast(i1 %c, float %x, double %y) {
  %1 = fptrunc double %y to float          ; original FPT had no FMF
  %r = select i1 %c, float %x, float %1    ; <-- 'fast' LOST
  ret float %r
}
```

## Analysis

The fpt path goes via `commonCastTransforms` → `FoldOpIntoSelect`. For one
arm, `simplifyOperationIntoSelectOperand` succeeds (`fptrunc(fpext X) -> X`).
For the other arm, `foldOperationIntoSelectOperand` clones the original cast.
The clone preserves the cast's FMF (see `Instruction::clone()`), which is
why `fptrunc ninf` survives. But the final `SelectInst::Create` does NOT
preserve the original select's FMF — `MDFrom` only carries metadata.

`select` FMF (added in 2022 as part of `select` having FP semantics) is
silently erased on this path.

## Severity

Soundness-preserving (refinement): the new select is at least as defined
as the original, but downstream passes that rely on the lost FMF (e.g.
`nsz` enabling `(-0) -> (+0)` canonicalization, or `nnan` enabling NaN
fast-paths) cannot fire on the new select.

The same erasure occurs anywhere `FoldOpIntoSelect` is used — there are
6 call sites across InstCombineCasts/Calls/MulDivRem/etc.

This sits at the intersection of the "sext/zext chained through select with
mismatched flags" theme from the hunt brief, but generalized to all
`select`-FMF rather than just integer-ext flags.

## Fix sketch

```cpp
auto *NewSI = SelectInst::Create(SI->getCondition(), NewTV, NewFV, "", nullptr, SI);
if (isa<FPMathOperator>(SI))
  NewSI->setFastMathFlags(SI->getFastMathFlags());
return NewSI;
```
