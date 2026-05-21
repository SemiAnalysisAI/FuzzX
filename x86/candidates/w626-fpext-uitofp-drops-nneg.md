# w626: visitFPExt / visitFPTrunc collapse uitofp into wider/narrower cast but drop `nneg`

## Source
- File: `llvm/lib/Transforms/InstCombine/InstCombineCasts.cpp`
- Function: `InstCombinerImpl::visitFPExt`, lines 2321-2330
- Function: `InstCombinerImpl::visitFPTrunc` at lines 2311-2316

## Code (visitFPExt)

```cpp
Instruction *InstCombinerImpl::visitFPExt(CastInst &FPExt) {
  Type *Ty = FPExt.getType();
  Value *Src = FPExt.getOperand(0);
  if (isa<SIToFPInst>(Src) || isa<UIToFPInst>(Src)) {
    auto *FPCast = cast<CastInst>(Src);
    if (isKnownExactCastIntToFP(*FPCast))
      return CastInst::Create(FPCast->getOpcode(), FPCast->getOperand(0), Ty);
    // ^ original UIToFP may carry nneg flag — silently dropped
  }
  return commonCastTransforms(FPExt);
}
```

The visitFPTrunc path at 2311-2316 has the identical pattern:

```cpp
Value *Src = FPT.getOperand(0);
if (isa<SIToFPInst>(Src) || isa<UIToFPInst>(Src)) {
  auto *FPCast = cast<CastInst>(Src);
  if (isKnownExactCastIntToFP(*FPCast))
    return CastInst::Create(FPCast->getOpcode(), FPCast->getOperand(0), Ty);
}
```

## Repro: `/tmp/icc625/uitofp-fpext.ll`

```llvm
target triple = "x86_64-unknown-linux-gnu"

define double @uitofp_ext(i8 %x) {
  %f = uitofp nneg i8 %x to float
  %d = fpext float %f to double
  ret double %d
}

define float @uitofp_trunc(i8 %x) {
  %f = uitofp nneg i8 %x to double
  %d = fptrunc double %f to float
  ret float %d
}
```

`opt -passes=instcombine -S`:

```llvm
define double @uitofp_ext(i8 %x) {
  %d = uitofp i8 %x to double    ; <-- nneg gone
  ret double %d
}

define float @uitofp_trunc(i8 %x) {
  %d = uitofp i8 %x to float     ; <-- nneg gone
  ret float %d
}
```

## Analysis

Per langref / [RFC discourse 77988](https://discourse.llvm.org/t/rfc-support-nneg-flag-with-uitofp/77988):
`uitofp nneg iN %x to fM` returns **poison** if `%x` is negative. Dropping
`nneg` makes the result well-defined for negative `%x` (large unsigned
float), which is a **refinement** — **soundness preserved**, but information
useful for downstream folding (e.g. equivalence with `sitofp`) is lost.

For a 1-byte input, this is largely cosmetic. For wider int inputs where
the int range fits in the FP mantissa, the loss can prevent
`fold uitofp nneg X => sitofp X` from firing in subsequent passes.

## Severity

Soundness-preserving missed optimization (refinement). Cited because the
hunt brief specifically asks about flag preservation through cast chains.

## Sources
- [RFC nneg uitofp](https://discourse.llvm.org/t/rfc-support-nneg-flag-with-uitofp/77988)
- [LLVM LangRef](https://llvm.org/docs/LangRef.html)
