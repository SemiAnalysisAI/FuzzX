# InstCombine visitFPExt drops FMF when sinking through (s|u)itofp

File: `llvm/lib/Transforms/InstCombine/InstCombineCasts.cpp` lines 2321-2332

```cpp
Instruction *InstCombinerImpl::visitFPExt(CastInst &FPExt) {
  Type *Ty = FPExt.getType();
  Value *Src = FPExt.getOperand(0);
  if (isa<SIToFPInst>(Src) || isa<UIToFPInst>(Src)) {
    auto *FPCast = cast<CastInst>(Src);
    if (isKnownExactCastIntToFP(*FPCast))
      return CastInst::Create(FPCast->getOpcode(), FPCast->getOperand(0), Ty);
  }
  return commonCastTransforms(FPExt);
}
```

## Reasoning

`FPExt` is an `FPMathOperator` and may carry fast-math flags
(`Operator.h` lines 364: `case Instruction::FPExt:` is in the FPMathOperator
classof). `SIToFP` / `UIToFP` are NOT in that switch, so the replacement
instruction created at line 2329 cannot carry FMF.

If the user wrote
```
%x.f = sitofp i64 %x to double
%y.f = fpext nnan ninf nsz double %x.f to fp128
```
this fold replaces it with
```
%y.f = sitofp i64 %x to fp128
```
which silently drops the `nnan/ninf/nsz/arcp/contract/afn/reassoc` markers.
In isolation that's fine because `sitofp` of an integer-representable value
is always a non-NaN/non-inf finite, but the FMF on the *fpext* may have been
used downstream by another optimization that proved a property using
intersection of FMF on its operands (`FPMathOperator::getFastMathFlags`
intersects up through chains in some folds). Losing FMF on a node that
participates in a wider expression of FP math can degrade later folds and,
in pathological cases of constrained-FP / strict-FP intermixing, change
observable behavior.

The companion fold in `visitFPTrunc` (lines 2311-2316) for
`fptrunc (sitofp X)` has the same property, plus an additional rounding
concern: `fptrunc double->float` of an exactly-converted integer rounds
once, but the fold replaces it with `sitofp i64 X to float` which also
rounds once — same rounding mode, same result. Safe in value, but the FMF
drop concern is identical.

## Repro

```
; opt -passes=instcombine -S
define fp128 @f(i64 %x) {
  %xf = sitofp i64 %x to double
  %r  = fpext nnan ninf double %xf to fp128
  ret fp128 %r
}
```
Expect: `sitofp i64 %x to fp128`, with FMF on the original fpext dropped.

## Expected wrong outcome

Conservative: missed-optimization downstream because FMF chain is broken.
Aggressive: when this fold runs inside an expression where another fold
later reads the FMF off `%r` (e.g. via FPMathOperator query while
constructing fmul/fadd with `%r`), the absence of `nnan` may suppress a
NaN-aware simplification that *would* have been valid with the original
FMF — turning a deterministic NaN into an output value.
