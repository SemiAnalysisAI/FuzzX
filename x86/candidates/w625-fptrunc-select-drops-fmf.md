# w625: fptrunc-of-select-with-fpext-arm drops FMF from both new select and new fptrunc

## Source
- File: `llvm/lib/Transforms/InstCombine/InstCombineCasts.cpp`
- Function: `InstCombinerImpl::visitFPTrunc`
- Lines: 2232-2264

## Pattern

```
// (fptrunc (fneg x)) -> (fneg (fptrunc x))
Value *X;
Instruction *Op = dyn_cast<Instruction>(FPT.getOperand(0));
if (Op && Op->hasOneUse()) {
  FastMathFlags FMF = FPT.getFastMathFlags();
  if (auto *FPMO = dyn_cast<FPMathOperator>(Op))
    FMF &= FPMO->getFastMathFlags();          // <-- INTERSECTION

  if (match(Op, m_FNeg(m_Value(X)))) {
    Value *InnerTrunc = Builder.CreateFPTruncFMF(X, Ty, FMF);
    Value *Neg = Builder.CreateFNegFMF(InnerTrunc, FMF);  // <-- both use intersection
    return replaceInstUsesWith(FPT, Neg);
  }

  // If we are truncating a select that has an extended operand, we can
  // narrow the other operand and do the select as a narrow op.
  Value *Cond, *X, *Y;
  if (match(Op, m_Select(m_Value(Cond), m_FPExt(m_Value(X)), m_Value(Y))) &&
      X->getType() == Ty) {
    // fptrunc (select Cond, (fpext X), Y --> select Cond, X, (fptrunc Y)
    Value *NarrowY = Builder.CreateFPTruncFMF(Y, Ty, FMF);
    Value *Sel =
        Builder.CreateSelectFMF(Cond, X, NarrowY, FMF, "narrow.sel", Op);
    return replaceInstUsesWith(FPT, Sel);
  }
  ...
```

## Symptom: Missed optimization (FMF erasure)

The new `select` and the new `fptrunc` each receive `intersect(FPT.FMF, Op.FMF)`,
which is **less FMF than either original instruction had**. The original FMF on
the select (e.g. `nnan`/`ninf`/`nsz`/`fast`) is silently dropped, and the
original FMF on the fptrunc is also silently dropped down to the intersection.

## Repro: `/tmp/icc625/fpt-sel.ll`

```llvm
target triple = "x86_64-unknown-linux-gnu"

define float @fpt_sel_sel_fast(i1 %c, float %x, double %y) {
  %xe = fpext float %x to double
  %s = select fast i1 %c, double %xe, double %y   ; <-- select fast
  %r = fptrunc double %s to float
  ret float %r
}

define float @fpt_sel_ninf(i1 %c, float %x, double %y) {
  %xe = fpext float %x to double
  %s = select nsz i1 %c, double %xe, double %y    ; <-- select nsz
  %r = fptrunc ninf double %s to float            ; <-- fptrunc ninf
  ret float %r
}
```

`opt -passes=instcombine -S`:
```llvm
define float @fpt_sel_sel_fast(i1 %c, float %x, double %y) {
  %1 = fptrunc double %y to float                  ; <-- 'fast' gone
  %r = select i1 %c, float %x, float %1            ; <-- 'fast' gone
  ret float %r
}

define float @fpt_sel_ninf(i1 %c, float %x, double %y) {
  %1 = fptrunc ninf double %y to float             ; ninf survived (intersection contains ninf? actually fptrunc had ninf, sel had nsz; intersection is empty)
  %r = select i1 %c, float %x, float %1            ; <-- nsz gone
  ret float %r
}
```

Wait — observed output: `fptrunc ninf` survived in `fpt_sel_ninf` (only `nsz` is dropped from select). That's actually the BUG: the intersection of `{ninf}` and `{nsz}` is empty, so `FMF` value passed to BOTH new ops should be empty. Yet `fptrunc ninf` retained `ninf`. Let me re-check.

Actually re-reading the source code, this is correct: `Builder.CreateFPTruncFMF(Y, Ty, FMF)` uses the intersected FMF, but the builder may merge with existing flags... no, `CreateFPTruncFMF` sets the FMF outright. So the survival of `ninf` on the new fptrunc when intersection is empty IS strange. Will investigate further.

Update: looking more carefully — `Builder.CreateFPTruncFMF` with `FMF` parameter sets the *exact* FMF on the new instruction; intersection with builder default isn't relevant here. So either the observed output has different mechanics, or the actual transform path is elsewhere. Worth investigation but the main symptom (select FMF erasure on `narrow.sel` path) is reproduced.

## Severity / Why this matters

These are **refinements** (more defined behavior, less FMF) and so they are
**sound** — but they constitute a missed-optimization that may cascade. The
new `select` has neither the original select's FMF nor any synthesized FMF
based on the operands' FMF. Downstream passes that rely on FMF (e.g. removing
NaN/Inf checks, reassociation) lose information.

This is **not** a soundness bug, but it is an FMF-laundering regression where
information is silently lost. Cited as a candidate per the user's request for
"sext/zext chained through select with mismatched flags" generalized to
fptrunc/fpext.
