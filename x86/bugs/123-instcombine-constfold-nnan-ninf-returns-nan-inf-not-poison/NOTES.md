# w64: ConstantFoldFPInstOperands + InstSimplify drop nnan/ninf, fold to NaN/Inf instead of poison

Files:
- llvm/lib/Analysis/ConstantFolding.cpp, `ConstantFoldFPInstOperands` ~ lines 1573-1610
- llvm/lib/Analysis/InstructionSimplify.cpp, `foldOrCommuteConstant` ~ lines 566-590 and `simplifyFDivInst`, `simplifyFAddInst`, etc.

## Reasoning

Per LangRef (reviews.llvm.org/D47963 + current LangRef "Fast-Math Flags"):
> If an instruction with the nnan or ninf flag set has an argument or a result that is a NaN or +/-inf, respectively, it produces a poison value.

When InstCombine constant-folds an FP binary op that has `nnan`/`ninf` flags set and the
mathematical result IS NaN / +/-inf, the fold currently returns a NaN/Inf constant rather
than `poison`. That is a refinement violation in the wrong direction:
the original instruction's nnan/ninf clause makes the IR-level result `poison`, but the
folded constant has a definite NaN/Inf value that downstream UB-detecting passes
(e.g. `nofpclass`, `assume(!isnan)`, branches gated on `fcmp ord`) will treat as well-defined.

`ConstantFoldFPInstOperands` (the helper that InstSimplify routes FP binops through) has only
two FMF-related guards:

```cpp
// If nsz or an algebraic FMF flag is set, the result of the FP operation
// may change due to future optimization. Don't constant fold them if
// non-deterministic results are not allowed.
if (!AllowNonDeterministic)
  if (auto *FP = dyn_cast_or_null<FPMathOperator>(I))
    if (FP->hasNoSignedZeros() || FP->hasAllowReassoc() ||
        FP->hasAllowContract() || FP->hasAllowReciprocal())
      return nullptr;

// ... do the fold ...

// The precise NaN value is non-deterministic.
if (!AllowNonDeterministic && C->isNaN())
  return nullptr;
```

Neither branch consults `FP->hasNoNaNs()` / `FP->hasNoInfs()`. The default
`AllowNonDeterministic = true` is what `simplifyFDivInst` (and friends) pass through
`foldOrCommuteConstant`, so even the NaN-payload bail doesn't run.

`simplifyFDivInst` does have an FMF-aware poison fold downstream:
```cpp
// nnan ninf X / [-]0.0 -> poison
if (FMF.noInfs() && match(Op1, m_AnyZeroFP()))
  return PoisonValue::get(Op1->getType());
```
but it requires `nnan && ninf` together and only matches divide-by-zero — it doesn't
cover `fdiv nnan 0.0, 0.0`, `fmul nnan inf, 0.0`, `fadd nnan inf, -inf`,
`fsub nnan inf, inf` (each of which the constant fold turns into a NaN constant before
the FMF logic ever fires), nor `fadd ninf max, max`, `fdiv ninf 1.0, 0.0` (which become
+Inf constants).

## Concrete IR (reproduces against the local x86 build)

```llvm
define float @nnan_div_zero() {
  %r = fdiv nnan float 0.0, 0.0
  ret float %r
}

define float @nnan_mul_inf_zero() {
  %r = fmul nnan float 0x7FF0000000000000, 0.0
  ret float %r
}

define double @nnan_add_inf_neginf() {
  %r = fadd nnan double 0x7FF0000000000000, 0xFFF0000000000000
  ret double %r
}

define double @nnan_sub_inf_inf() {
  %r = fsub nnan double 0x7FF0000000000000, 0x7FF0000000000000
  ret double %r
}

define float @ninf_div_one_zero() {
  %r = fdiv ninf float 1.0, 0.0
  ret float %r
}

define double @ninf_add_max_max() {
  %r = fadd ninf double 0x7FEFFFFFFFFFFFFF, 0x7FEFFFFFFFFFFFFF
  ret double %r
}
```

`opt -passes=instcombine -S`:

```llvm
define float @nnan_div_zero() {
  ret float +qnan          ; expected: poison
}

define float @nnan_mul_inf_zero() {
  ret float +qnan          ; expected: poison
}

define double @nnan_add_inf_neginf() {
  ret double +qnan         ; expected: poison
}

define double @nnan_sub_inf_inf() {
  ret double +qnan         ; expected: poison
}

define float @ninf_div_one_zero() {
  ret float +inf           ; expected: poison
}

define double @ninf_add_max_max() {
  ret double +inf          ; expected: poison
}
```

## Miscompile angle

A downstream consumer assumed by nnan/ninf is allowed to treat the result as not-NaN /
finite — e.g. an `fcmp ord` that always returns true, an `assume(!isnan)`, or a branch on
`fcmp oeq %r, %r`. Constant-propagating a literal NaN through that consumer keeps the IR
"defined" with the NaN value, then a later pass that proves `%r != nan` (using the nnan
attribute on a use chain or a `nofpclass` return) can fold the consumer in a way that
contradicts the literal NaN bits. That mismatch is the textbook source of nnan/ninf
miscompiles (cf. discourse "nnan, ninf, and poison" threads).

The fix is for `ConstantFoldFPInstOperands` (or its InstSimplify wrapper) to inspect
`FP->hasNoNaNs()` / `FP->hasNoInfs()` on the context instruction and return
`PoisonValue::get(Ty)` instead of the NaN/Inf constant.
