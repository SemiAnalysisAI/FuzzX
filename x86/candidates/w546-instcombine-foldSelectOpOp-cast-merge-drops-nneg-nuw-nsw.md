# InstCombine `foldSelectOpOp` cast-merge drops nneg/nuw/nsw flags on the new cast

## Summary

The fold

```
select c, (cast0 X), (cast0 Y)  -->  cast0 (select c, X, Y)
```

in `InstCombinerImpl::foldSelectOpOp` builds the new cast with
`CastInst::Create(...)` and does not intersect / preserve `nneg`, `nuw`, `nsw`
from the two original casts. When both source casts agree on a flag, the
resulting cast loses it.

## Source citation

`llvm/lib/Transforms/InstCombine/InstCombineSelect.cpp`:

```cpp
// 319    // Fold this by inserting a select from the input values.
// 320    Value *NewSI =
// 321        Builder.CreateSelect(Cond, TI->getOperand(0), FI->getOperand(0),
// 322                             SI.getName() + ".v", &SI);
// 323    return CastInst::Create(Instruction::CastOps(TI->getOpcode()), NewSI,
// 324                            TI->getType());
```

No call site copies / intersects flags from `TI` and `FI` onto the returned
cast. Contrast with nearby calls (e.g. `foldBitOpOfCastops` in VectorCombine
`Vectorize/VectorCombine.cpp:955-957`) that do `copyIRFlags(LHSCast);
andIRFlags(RHSCast);` to preserve the intersection.

## Reproducer (x86, opt -O2 / instcombine)

```llvm
; RUN: opt -S -passes=instcombine
define i32 @f(i1 %c, i16 %x, i16 %y) {
  %tx = zext nneg i16 %x to i32
  %ty = zext nneg i16 %y to i32
  %s = select i1 %c, i32 %tx, i32 %ty
  ret i32 %s
}
```

Result:

```llvm
define i32 @f(i1 %c, i16 %x, i16 %y) {
  %s.v = select i1 %c, i16 %x, i16 %y
  %s = zext i16 %s.v to i32   ; lost: nneg (was nneg on both arms)
  ret i32 %s
}
```

The new `zext` should be `zext nneg` because both source operands have
`nneg`, and `select(c, nneg_X, nneg_Y)` is itself non-negative.

The same loss applies to `sext nneg`, `trunc nuw`, and `trunc nsw` when both
arms share the flag.

## Impact

Missed optimization. `nneg` enables the cast to be treated as an `sext` for
known-bits analysis, which can unlock further folds (e.g. `ult` of an `nneg
zext` value can use signed comparison).

## Severity

Quality. Not a miscompile.
