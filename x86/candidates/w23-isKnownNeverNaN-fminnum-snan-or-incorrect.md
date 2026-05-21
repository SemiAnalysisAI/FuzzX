# isKnownNeverNaN: FMINNUM/FMAXNUM (and *NUM variants) OR-logic is wrong for SNaN=true

**File:** `llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp:6346-6353`

## Code
```cpp
case ISD::FMINNUM:
case ISD::FMAXNUM:
case ISD::FMINIMUMNUM:
case ISD::FMAXIMUMNUM: {
  // Only one needs to be known not-nan, since it will be returned if the
  // other ends up being one.
  return isKnownNeverNaN(Op.getOperand(0), DemandedElts, SNaN, Depth + 1) ||
         isKnownNeverNaN(Op.getOperand(1), DemandedElts, SNaN, Depth + 1);
}
```

## Reasoning

LangRef for `llvm.minnum`: "If either operand is a NaN, returns the other
non-NaN operand. Returns NaN only if both operands are NaN." The OR-logic is
sound when `SNaN == false` (the boolean meaning "known never any NaN") — if at
least one operand is known to be non-NaN, the result cannot be NaN. However the
same recursion with `SNaN == true` (meaning "known never *signaling* NaN —
QNaN is permitted") is unsound:

Counter-example: op0 is known never SNaN but may be a QNaN; op1 is unknown
(could be SNaN). Per the recursion, `isKnownNeverNaN(op0, SNaN=true)` returns
**true** and the whole expression returns **true**. But at runtime if op0 = QNaN
and op1 = SNaN both operands are NaNs, so per LangRef the result is "a NaN" —
the implementation typically returns one of the operands unchanged (no
quieting); the result therefore can be the SNaN operand. So `isKnownNeverSNaN`
incorrectly claims FMINNUM never produces an SNaN.

Contrast with the *adjacent* `FMINNUM_IEEE`/`FMAXNUM_IEEE` case (lines 6355-6364)
which correctly handles SNaN: returns `true` unconditionally for SNaN=true, on
the basis that IEEE-754 minNum quiets all NaN operands. The non-IEEE
`FMINNUM`/`FMAXNUM` (libm fmin/fmax) and the IEEE-2019 `FMINIMUMNUM`/
`FMAXIMUMNUM` do *not* quiet, so the OR-logic should be an AND when SNaN=true
(or just `return false` when SNaN=true and we cannot prove both never-NaN).

## Downstream effect

`SelectionDAG::isKnownNeverSNaN()` is used by DAGCombiner
(`arebothOperandsNotSNan`, lines 6735-6738) to gate FMINNUM_IEEE / FMAXNUM_IEEE
operand reordering / fold safety, and by `getMinMaxOpcodeForCompareFold` (line
6761). A false-positive here means a fold treats a potentially-SNaN value as
quiet, and the resulting IEEE-min/max may convert that SNaN to QNaN at runtime
where the original IR would have propagated it as-is — observable when the
program checks `signaling_nan()` via `bitcast f32 to i32` or via FP exception
flags.

## Repro sketch (IR)

```llvm
; Pattern: result of fminnum is later fed into a context that calls
; isKnownNeverSNaN (e.g., FMAXNUM_IEEE combining).
define float @t(float %a, float %b, float %c) {
  ; %a is known never-SNaN (but may be QNaN) — e.g. via assertion node.
  %na = call float @llvm.canonicalize.f32(float %a)  ; QNaN preserves QNaN-ness, SNaN -> QNaN
  %m  = call float @llvm.minnum.f32(float %na, float %b)
  ; combiner asks: is %m known never SNaN?  Per buggy logic: yes (because %na is).
  ; But if %na = QNaN and %b = SNaN, %m may be %b = SNaN.
  %r  = call float @llvm.maxnum.ieee.f32(float %m, float %c)
  ret float %r
}
```

## Expected wrong outcome

A combiner that treats `%m` as never-SNaN may eliminate a `canonicalize`/quieten
operation around `%m`, allowing an SNaN bit pattern to reach a context (e.g.,
final store to memory, comparison) where the source IR would have produced a
QNaN. Detectable by checking signaling-NaN-ness of the result with
`bitcast f32 to i32` and comparing against the SNaN payload.

## Fix sketch

For the `*NUM` variants (FMINNUM/FMAXNUM/FMINIMUMNUM/FMAXIMUMNUM), split:
```cpp
if (!SNaN)
  return isKnownNeverNaN(op0, ..., false, Depth+1) ||
         isKnownNeverNaN(op1, ..., false, Depth+1);
// SNaN: need to prove neither input is SNaN (because both-NaN may pass SNaN through).
return isKnownNeverNaN(op0, ..., true, Depth+1) &&
       isKnownNeverNaN(op1, ..., true, Depth+1);
```
