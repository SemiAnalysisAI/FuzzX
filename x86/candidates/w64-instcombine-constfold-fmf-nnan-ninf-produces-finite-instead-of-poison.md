# w64: ConstantFoldFPInstOperands / ConstantFoldFP ignore nnan/ninf, fold to NaN or Inf instead of poison

Files:
- llvm/lib/Analysis/ConstantFolding.cpp
  - `ConstantFoldFPInstOperands` ~ lines 1573-1610 (fadd/fsub/fmul/fdiv/frem constant fold)
  - `ConstantFoldFP` ~ lines 2263-2284 (unary FP intrinsics: sqrt, log, log2, exp, etc.)
- Callers in llvm/lib/Analysis/InstructionSimplify.cpp (`foldOrCommuteConstant`,
  `simplifyFDivInst`, etc.) and llvm/lib/Transforms/InstCombine/* (visitFDiv, visitCallInst).

## Reasoning

Per LangRef (Fast-Math Flags) and the original spec change
(reviews.llvm.org/D47963 -- `[LangRef] nnan and ninf produce poison`):

> If an instruction with the nnan or ninf flag set has an argument or a result that is a NaN
> or +/-inf, respectively, it produces a poison value.

That is a contract on the IR-level result: when constant folding shows the result IS NaN
or +/-Inf, an instruction carrying the corresponding fast-math flag must fold to `poison`,
not to the NaN/Inf constant. Today InstCombine routes FP binops and most FP intrinsics
through two helpers that ignore nnan/ninf:

`ConstantFoldFPInstOperands` only consults FMF for these two narrow cases:
```cpp
if (!AllowNonDeterministic)
  if (auto *FP = dyn_cast_or_null<FPMathOperator>(I))
    if (FP->hasNoSignedZeros() || FP->hasAllowReassoc() ||
        FP->hasAllowContract() || FP->hasAllowReciprocal())
      return nullptr;
// ... compute C = ConstantFoldBinaryOpOperands(Opcode, Op0, Op1, DL) ...
if (!AllowNonDeterministic && C->isNaN())
  return nullptr;
```
Neither branch tests `FP->hasNoNaNs()` or `FP->hasNoInfs()`. The default
`AllowNonDeterministic = true` is what InstSimplify uses (see
`foldOrCommuteConstant` in InstructionSimplify.cpp), so even the NaN-payload guard
never fires.

`ConstantFoldFP` for unary intrinsics (`sqrt`, `log`, `log2`, `log10`, `exp`, `exp2`,
`sin`, `cos`, `tan`, `atan`, `cosh`, `sinh`, ...) is entirely flag-agnostic:
```cpp
Constant *ConstantFoldFP(double (*NativeFP)(double), const APFloat &V, Type *Ty, ...) {
  ...
  double Result = NativeFP(Input.convertToDouble());
  ...
  return GetConstantFoldFPValue(Result, Ty);
}
```

`simplifyFDivInst` has a *single* FMF-gated poison fold but it requires both `nnan` and
`ninf` and only matches `_ / 0.0`:
```cpp
// nnan ninf X / [-]0.0 -> poison
if (FMF.noInfs() && match(Op1, m_AnyZeroFP()))
  return PoisonValue::get(Op1->getType());
```
So plain `fdiv nnan 0.0, 0.0` (no `ninf`) and every other shape miss the guard.

## Concrete IR — all reproduced against the local build

```llvm
declare float @llvm.sqrt.f32(float)
declare float @llvm.log.f32(float)
declare float @llvm.fma.f32(float, float, float)

define float @nnan_fdiv_zero_zero() {
  %r = fdiv nnan float 0.0, 0.0
  ret float %r
}

define float @nnan_fmul_inf_zero() {
  %r = fmul nnan float 0x7FF0000000000000, 0.0
  ret float %r
}

define double @nnan_fadd_inf_neginf() {
  %r = fadd nnan double 0x7FF0000000000000, 0xFFF0000000000000
  ret double %r
}

define double @nnan_fsub_inf_inf() {
  %r = fsub nnan double 0x7FF0000000000000, 0x7FF0000000000000
  ret double %r
}

define float @nnan_frem_one_zero() {
  %r = frem nnan float 1.0, 0.0
  ret float %r
}

define float @nnan_fneg_nan() {
  %r = fneg nnan float 0x7FF8000000000000
  ret float %r
}

define float @nnan_sqrt_neg() {
  %r = call nnan float @llvm.sqrt.f32(float -1.0)
  ret float %r
}

define float @nnan_log_neg() {
  %r = call nnan float @llvm.log.f32(float -1.0)
  ret float %r
}

define float @nnan_fma_zero_inf_zero() {
  %r = call nnan float @llvm.fma.f32(float 0.0, float 0x7FF0000000000000, float 0.0)
  ret float %r
}

define float @ninf_fdiv_one_zero() {
  %r = fdiv ninf float 1.0, 0.0
  ret float %r
}

define double @ninf_fadd_max_max() {
  %r = fadd ninf double 0x7FEFFFFFFFFFFFFF, 0x7FEFFFFFFFFFFFFF
  ret double %r
}
```

`opt -passes=instcombine -S` returns:

```llvm
define float @nnan_fdiv_zero_zero() { ret float +qnan }   ; expected: poison
define float @nnan_fmul_inf_zero()  { ret float +qnan }   ; expected: poison
define double @nnan_fadd_inf_neginf() { ret double +qnan } ; expected: poison
define double @nnan_fsub_inf_inf()  { ret double +qnan }   ; expected: poison
define float @nnan_frem_one_zero()  { ret float +qnan }    ; expected: poison
define float @nnan_fneg_nan()       { ret float -qnan }    ; expected: poison (nnan + NaN input)
define float @nnan_log_neg()        { ret float +qnan }    ; expected: poison
define float @nnan_fma_zero_inf_zero() { ret float +qnan } ; expected: poison
define float @ninf_fdiv_one_zero()  { ret float +inf }     ; expected: poison
define double @ninf_fadd_max_max()  { ret double +inf }    ; expected: poison
```

(`nnan_sqrt_neg` happens to be left as a `call nnan float @llvm.sqrt.f32(float -1.0)` by
InstCombine because the unary-FP fold path is a different code path that also doesn't honor
nnan; codegen will still produce a NaN bit pattern at runtime, with downstream consumers
free to treat it as either NaN or `assume(!isnan)`.)

## Why this is a real miscompile, not just a "stale flag"

The two passes that both consume the result come to contradictory conclusions:

```llvm
; A. constant-fold first  ->  fcmp ord NaN, 0  ->  false
define i1 @direct() {
  %r = fdiv nnan float 0.0, 0.0
  %c = fcmp ord float %r, 0.0
  ret i1 %c           ; instcombine: ret i1 false
}

; B. nnan-aware fold first ->  fcmp ord r, 0  with r-not-NaN -> true
define i1 @via_sqrt() {
  %r = call nnan float @llvm.sqrt.f32(float -1.0)
  %c = fcmp ord float %r, 0.0
  ret i1 %c           ; instcombine: ret i1 true
}
```

Both functions describe the same poison value semantically (an nnan op whose result is NaN),
but the constant-folder picked `NaN` and the nnan-aware simplifier picked `not-NaN`. A second
pass that examines the post-fold IR (e.g. KnownFPClass, ValueTracking using the `nnan`
attribute on a call return, `nofpclass` on a use) will disagree with the literal NaN bits
the constant-folder left behind. That mismatch is precisely the source of the nnan
miscompiles tracked at discourse "nnan, ninf, and poison" (D47963) and llvm-project
issue tracker around `__builtin_isnan`/`-ffinite-math-only`.

The fix is to teach `ConstantFoldFPInstOperands` and `ConstantFoldFP` to inspect
`FPMathOperator::hasNoNaNs()` / `hasNoInfs()` on the context instruction and return
`PoisonValue::get(Ty)` when the computed result trips the corresponding flag, instead
of returning the NaN/Inf constant.
