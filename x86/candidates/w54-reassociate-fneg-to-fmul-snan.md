# w54: Reassociate lowers unary `fneg` to `fmul x, -1.0`, quieting sNaN

**File:lines:** `llvm/lib/Transforms/Scalar/Reassociate.cpp:289-305` (`LowerNegateToMultiply`); reached from `OptimizeInst` lines 2226-2253 (FNeg path).

## Reasoning

`LowerNegateToMultiply` rewrites a unary `fneg %x` into `fmul %x, -1.0`. The
source itself carries an inline FIXME:

```cpp
// FIXME: It's not safe to lower a unary FNeg into a FMul by -1.0.
unsigned OpNo = isa<BinaryOperator>(Neg) ? 1 : 0;
Type *Ty = Neg->getType();
Constant *NegOne = Ty->isIntOrIntVectorTy() ?
  ConstantInt::getAllOnesValue(Ty) : ConstantFP::get(Ty, -1.0);
BinaryOperator *Res =
    CreateMul(Neg->getOperand(OpNo), NegOne, "", Neg->getIterator(), Neg);
```

The unary `llvm.fneg` semantic per LangRef is purely a sign-bit flip — sNaN
operands are *not* quieted. `fmul x, -1.0` without `nnan` is a real
multiplication that does quiet sNaN on every IEEE-conforming hardware (and on
x86 specifically, FMUL lowers to `mulss`/`mulsd`/`vmulss` which quiet sNaN
even though FNEG would lower to `xorps`).

`OptimizeInst` reaches this lowering from `Instruction::FNeg` /
`Instruction::FSub` arms (lines 2226-2253) whenever the FNeg's operand is a
reassociable FMul (i.e. an FMul carrying `reassoc nsz`). The check is on the
*operand* FMul's FMF, but the FNeg itself can be a vanilla unary `fneg` with
no FMF whatsoever, so the lowering happens even when the user explicitly did
not opt into NaN-quieting fast-math semantics. `CreateMul` (line 263-274)
then attaches the FNeg's FMF to the new fmul (`FMFSource = Neg`), so the
constructed `fmul %x*%y, -1.0` carries only the FNeg's (typically empty) FMF.

For a signaling-NaN intermediate this is a miscompile: the original `fneg`
preserves the signaling bit; the rewritten `fmul %t, -1.0` quiets it.

## Candidate IR

```ll
define float @f(float %a, float %b) {
  %m  = fmul reassoc nsz float %a, %b        ; reassociable
  %n  = fneg float %m                        ; unary fneg, no fmf
  ; later use that depends on the signaling bit; e.g.,
  ; passing %n into an FE_INVALID-trapping fenv region, or to a
  ; runtime that examines payload via bitcast.
  ret float %n
}
```

With `%a*%b` evaluating to an sNaN at runtime (e.g. `0x7fa00000`), the
original IR returns sNaN with sign flipped (`0xffa00000`). After Reassociate
runs, `%n` becomes `fmul %m, -1.0`; on x86 this lowers to `mulss` which
quiets the result to `0xffe00000` (qNaN). Bit-pattern observable via
`bitcast float to i32`.

## Confirmation status

Source-confirmed. Inline FIXME at line 292 documents the unsoundness.

## Next step

Construct concrete `.ll` repro, run `opt -passes=reassociate` and observe
that `fneg %m` becomes `fmul %m, -1.0` in the resulting IR. Then `llc -O0`
and compare with hand-emitted `xorps` for the original.
