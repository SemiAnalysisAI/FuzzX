# 234 — InstructionSimplify `simplifyFPOp` folds `llvm.experimental.constrained.*` with FMF poison-class operand, eliding `fpexcept.strict` side-effect

Component: `llvm/lib/Analysis/InstructionSimplify.cpp` lines ~5897-5900

`simplifyFPOp` returns `PoisonValue::get(...)` when a constrained FP call carries FMF `nnan`/`ninf` and any operand is a NaN/Inf constant. The early-return does NOT consult `ExBehavior` (`fpexcept.strict` vs `ignore`). When the call was `fpexcept.strict`, an IEEE-754 Invalid Operation exception that should be raised at runtime is silently elided as the call disappears.

The subsequent `propagateNaN` branches at lines 5902-5914 DO consult `ExBehavior` — the asymmetry shows the poison fold should too.

## Reproducer

`opt -passes=instsimplify -S repro.ll`:

Input: `tail call nnan constrained.fadd(sNaN, 1.0, "round.dynamic", "fpexcept.strict") strictfp` — should raise FE_INVALID.
Output: the call is gone, `ret double +nan(0x1)`. The strict-FP exception is lost; the function is now nominally pure.

## Severity

Strict-FP code that relies on exception delivery (e.g., `fetestexcept` after the operation) is silently miscompiled.

## Fix

Gate the `PoisonValue::get(...)` return on `ExBehavior == fp::ebIgnore`. For strict variants, fall through to a path that preserves the call's side-effect.
