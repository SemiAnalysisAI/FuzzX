# simplifyFPOp drops strict-FP exception side effect when FMF (nnan/ninf) folds a constrained intrinsic to poison

## Summary
`simplifyFPOp` (InstructionSimplify.cpp:5880-5917) folds a math op to
`PoisonValue` whenever the FMF promises `nnan`/`ninf` AND an operand is a
NaN/Inf constant or undef. The check is **unconditional on
`ExceptionBehavior`/`RoundingMode`**: a `llvm.experimental.constrained.fadd`
carrying `fpexcept.strict` is dropped to `poison` just like a plain `fadd`.

The strict variant is required to raise FP exceptions (Invalid for sNaN+x,
Invalid for +Inf+(-Inf)). By collapsing the call to `poison`, InstCombine
deletes the call and drops the `memory(inaccessiblemem: readwrite)` side
effect (which encodes the exception-raising behavior). Subsequent
`fetestexcept(FE_INVALID)` will return 0 where IEEE-754 mandates the flag be
set.

## Reproducer 1: nnan + SNaN, fpexcept.strict
```llvm
define double @t1() strictfp {
entry:
  %r = call nnan double @llvm.experimental.constrained.fadd.f64(
    double 0x7FF0000000000001,                ; sNaN payload
    double 1.0,
    metadata !"round.dynamic",
    metadata !"fpexcept.strict") strictfp
  ret double %r
}
declare double @llvm.experimental.constrained.fadd.f64(double, double, metadata, metadata)
```

`opt -O2 -S`:
```llvm
; Function Attrs: ... memory(none)
define double @t1() local_unnamed_addr #0 {
entry:
  ret double poison
}
```

Note `memory(inaccessiblemem: readwrite)` (which is how strictfp encodes the
side-effect that constrained-FP env+exception state is read/written) becomes
`memory(none)`, and the call is gone. A user running this with
`feenableexcept(FE_INVALID)` no longer sees the SIGFPE that the source
program demanded.

Without `nnan`, the call is preserved verbatim — confirming the FMF flag is
what triggers the fold, even on the strict variant.

## Reproducer 2: ninf + +Inf, fpexcept.strict (unknown other operand)
```llvm
define double @t3(double %x) strictfp {
entry:
  %r = call ninf double @llvm.experimental.constrained.fadd.f64(
    double 0x7FF0000000000000,                ; +Inf
    double %x,
    metadata !"round.dynamic",
    metadata !"fpexcept.strict") strictfp
  ret double %r
}
declare double @llvm.experimental.constrained.fadd.f64(double, double, metadata, metadata)
```
`opt -O2 -S` yields the same `ret double poison; memory(none)`. The
`ninf` flag is a user promise that no infinity will appear at runtime —
i.e. the user already broke their own promise (since one operand is +Inf
unconditionally), so producing poison is defensible from a value perspective.
But the strict-exception contract is orthogonal: if `%x == -Inf`,
`fadd(+Inf, -Inf)` raises Invalid, and the IR loses that observable.

Equivalent triggering operations: `constrained.fsub`, `constrained.fmul`,
`constrained.fdiv`, `constrained.frem`, `constrained.fma` — all route
through `simplifyFPOp` from InstructionSimplify.cpp:7390-7419.

## Root cause
`llvm/lib/Analysis/InstructionSimplify.cpp:5880-5917`:
```c++
static Constant *simplifyFPOp(ArrayRef<Value *> Ops, FastMathFlags FMF,
                              const SimplifyQuery &Q,
                              fp::ExceptionBehavior ExBehavior,
                              RoundingMode Rounding) {
  // Poison is independent of anything else. ...
  if (any_of(Ops, IsaPred<PoisonValue>))
    return PoisonValue::get(Ops[0]->getType());

  for (Value *V : Ops) {
    bool IsNan = match(V, m_NaN());
    bool IsInf = match(V, m_Inf());
    bool IsUndef = Q.isUndefValue(V);

    // If this operation has 'nnan' or 'ninf' and at least 1 disallowed operand
    // (an undef operand can be chosen to be Nan/Inf), then the result of
    // this operation is poison.
    if (FMF.noNaNs() && (IsNan || IsUndef))
      return PoisonValue::get(V->getType());        // <-- no ExBehavior check
    if (FMF.noInfs() && (IsInf || IsUndef))
      return PoisonValue::get(V->getType());        // <-- no ExBehavior check
    ...
  }
}
```
The two poison-returning branches do not consult `ExBehavior` even though
both branches in the *subsequent* `isDefaultFPEnvironment` block (lines
5902-5914) do consult it for NaN propagation. The constrained-FP callers at
lines 7390-7419 pass `*FPI->getExceptionBehavior()` precisely so this
function can honor it — but these two branches ignore the parameter.

## Fix sketch
Gate the two `return PoisonValue::get(...)` returns on
`isDefaultFPEnvironment(ExBehavior, Rounding)`, or at minimum on
`ExBehavior != fp::ebStrict`. With `fpexcept.strict`, even a poison value
folder must keep the call alive so codegen lowers a real instruction that
raises the IEEE flag.

(Related but distinct: w64 covers the same simplifyFPOp poison-fold path
for non-strict `fadd`/`fcmp` constant folding. w53/w60 cover sNaN bit-leak
through `x*1.0` style identity folds. None reaches the strict-side-effect
issue.)
