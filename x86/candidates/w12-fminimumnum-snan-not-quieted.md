# w12: FMINIMUMNUM(X, qNaN) -> X drops sNaN-quieting requirement

**File:lines:** `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:20498-20516` (visitFMinMax)

## Reasoning

Inside `visitFMinMax`, when N1 is a constant NaN, the code does:

```cpp
if (AF.isNaN()) {
  if (PropAllNaNsToQNaNs || (AF.isSignaling() && PropOnlySNaNsToQNaNs)) {
    if (AF.isSignaling())
      return DAG.getConstantFP(AF.makeQuiet(), SDLoc(N), VT);
    return N->getOperand(1);
  }
  return N->getOperand(0);   // <-- here
}
```

For `ISD::FMINIMUMNUM` / `ISD::FMAXIMUMNUM`, `PropAllNaNsToQNaNs` is false and
`PropOnlySNaNsToQNaNs` is false, so the code falls into `return N0` whenever N1
is any NaN (including a non-signaling NaN). Per IEEE 754-2019
`minimumNumber(x, y)`: if **both** operands are NaN, the result must be a qNaN.
The transform `minimumnum(X, qNaN) -> X` is therefore only valid when X is known
non-NaN (e.g. `nnan` on N or N0), but the code performs it unconditionally.

The wrong-direction case: if X happens to be a **signaling** NaN at runtime, the
correct minimumNumber result is a quiet NaN (sNaN payload should be quieted),
but the transform returns the original sNaN bits unchanged.

## Candidate IR

```ll
; minimumnum(sNaN_var, qNaN_const) per IEEE 754-2019 must yield qNaN.
; DAGCombiner replaces the call with just %x, so an sNaN payload survives.
define i64 @f(double %x) {
  %r = call double @llvm.minimumnum.f64(double %x, double 0x7FF8000000000000)
  %i = bitcast double %r to i64
  ret i64 %i
}
declare double @llvm.minimumnum.f64(double, double)
```

`llc -mtriple=x86_64` produces:
```
f:
  movq %xmm0, %rax       ; just returns x as-is
  retq
```

## Expected wrong outcome

Caller passes `0x7FF0000000000001` (signaling NaN). The minimumnum semantics
require returning a qNaN (the quieting bit set, e.g. `0x7FF8000000000001`), but
the generated code returns the raw input `0x7FF0000000000001`. A user testing
sNaN handling on x86 would observe a signaling-NaN bit pattern surviving past a
`llvm.minimumnum` call.

Same applies to `llvm.maximumnum`. The companion `FMINNUM`/`FMAXNUM` and
`FMINIMUM`/`FMAXIMUM` cases are handled correctly above; only the
`MINIMUMNUM`/`MAXIMUMNUM` fallthrough is wrong.
