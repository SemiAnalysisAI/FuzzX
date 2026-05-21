# w64: ConstantFoldCompareInstOperands ignores fcmp nnan/ninf; folds NaN/Inf-operand compares to true/false instead of poison

Files:
- llvm/lib/Analysis/ConstantFolding.cpp, `ConstantFoldCompareInstOperands` (around the FCmp dispatch in lines that switch on `Instruction::FCmp`).
- llvm/lib/Analysis/InstructionSimplify.cpp, `simplifyFCmpInst` ~ lines 4159-4220: constant fold at line 4167 dominates and runs before the FMF-aware ord/uno paths at 4209-4218.

## Reasoning

`fcmp` carries fast-math flags too. Per LangRef (Fast-Math Flags, D47963):
"If an instruction with the nnan or ninf flag set has an argument or a result that is a NaN
or +/-inf, respectively, it produces a poison value."

`fcmp nnan <pred> X, Y` therefore produces `poison` whenever `X` or `Y` is NaN. Likewise
`fcmp ninf <pred>` is poison whenever an operand is +/-Inf. When both operands are constants
and one is a NaN, `simplifyFCmpInst` short-circuits to constant fold at the top of the function:

```cpp
static Value *simplifyFCmpInst(CmpPredicate Pred, Value *LHS, Value *RHS,
                               FastMathFlags FMF, const SimplifyQuery &Q,
                               unsigned MaxRecurse) {
  ...
  if (Constant *CLHS = dyn_cast<Constant>(LHS)) {
    if (Constant *CRHS = dyn_cast<Constant>(RHS)) {
      // if the folding isn't successful, fall back to the rest of the logic
      if (auto *Result = ConstantFoldCompareInstOperands(Pred, CLHS, CRHS, Q.DL,
                                                         Q.TLI, Q.CxtI))
        return Result;
    }
    ...
  }
  ...
}
```

`ConstantFoldCompareInstOperands` does not consult `FMF`, so the post-constant FMF-aware
folds (lines 4209-4218 that say "Fold (un)ordered comparison if we can determine there are
no NaNs") never get a chance to widen the result to `poison`. The constant-folded boolean
is returned directly.

## Concrete IR (reproduces against the local build)

```llvm
define i1 @nnan_fcmp_oeq_nan() {
  %r = fcmp nnan oeq float 0x7FF8000000000000, 1.0
  ret i1 %r
}

define i1 @nnan_fcmp_ord_nan() {
  %r = fcmp nnan ord float 0x7FF8000000000000, 1.0
  ret i1 %r
}

define i1 @nnan_fcmp_uno_nan() {
  %r = fcmp nnan uno float 0x7FF8000000000000, 1.0
  ret i1 %r
}

define i1 @ninf_fcmp_olt_inf() {
  %r = fcmp ninf olt float 0x7FF0000000000000, 1.0
  ret i1 %r
}
```

`opt -passes=instcombine -S`:

```llvm
define i1 @nnan_fcmp_oeq_nan() { ret i1 false }   ; expected: poison
define i1 @nnan_fcmp_ord_nan() { ret i1 false }   ; expected: poison
define i1 @nnan_fcmp_uno_nan() { ret i1 true }    ; expected: poison
define i1 @ninf_fcmp_olt_inf() { ret i1 false }   ; expected: poison
```

## Miscompile angle

Returning a definite `false` for `fcmp nnan ord NaN, 1.0` while a parallel path (e.g.
`fcmp nnan ord %x, 1.0` where `%x` is the same NaN-producing value reached via
`computeKnownFPClass` + the nnan-aware fold at 4213) folds the same poison value to
`true`. Two consumers see opposite booleans for the same IR-level poison, opening the
same divergence class as the binop nnan/ninf constant-fold bug filed in the companion
candidate (`w64-instcombine-constfold-fmf-nnan-ninf-produces-finite-instead-of-poison.md`).
The fix is the same: have the FP constant-folder check `FPMathOperator::hasNoNaNs()` /
`hasNoInfs()` on the context instruction (for `fcmp`, on the `FCmpInst` itself) and return
`PoisonValue::get(Ty)` when an operand trips the corresponding flag, instead of returning
the bool the IEEE 754 predicate would have given.
