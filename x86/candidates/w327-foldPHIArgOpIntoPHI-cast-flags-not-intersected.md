# w327: foldPHIArgOpIntoPHI's CastInst path drops IR flags entirely

## Summary

Same shape of bug as w326 but in the sister CastInst arm of
`foldPHIArgOpIntoPHI`. When the per-predecessor casts all share the same
poison-generating flag (`zext nneg`, `trunc nuw`, `trunc nsw`), the merged
cast that replaces the PHI gets created with no flags at all.

## Source

`llvm/lib/Transforms/InstCombine/InstCombinePHI.cpp:953-958`:

```
953    if (CastInst *FirstCI = dyn_cast<CastInst>(FirstInst)) {
954      CastInst *NewCI = CastInst::Create(FirstCI->getOpcode(), PhiVal,
955                                         PN.getType());
956      PHIArgMergedDebugLoc(NewCI, PN);
957      return NewCI;                            ; <-- no copy/andIRFlags
958    }
```

For comparison, the BinaryOperator arm directly below correctly does
`NewBinOp->copyIRFlags(BinOp);` (`InstCombinePHI.cpp:960-964`). The CastInst
arm is the inconsistent one.

`copyIRFlags` already handles cast-specific flags:
- `TruncInst` nuw/nsw: `llvm/lib/IR/Instruction.cpp:730-735`
- `PossiblyNonNegInst` (zext nneg, uitofp nneg): `Instruction.cpp:756-758`
- `PossiblyDisjointInst` (or disjoint): `Instruction.cpp:742-744`
  (not a CastInst, but illustrates the pattern is general)

## Reproducers (both diff after `opt -passes=instcombine -S`):

### zext nneg drop
```llvm
target datalayout = "e-m:e-p:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i64 @test(i1 %c, i32 %x, i32 %y) {
entry:
  br i1 %c, label %bb1, label %bb2
bb1:
  %a = zext nneg i32 %x to i64
  br label %end
bb2:
  %b = zext nneg i32 %y to i64
  br label %end
end:
  %p = phi i64 [ %a, %bb1 ], [ %b, %bb2 ]
  ret i64 %p
}
```
After: `%p = zext i32 %p.in to i64`  (missing `nneg`)

### trunc nuw nsw drop
```llvm
define i16 @test(i1 %c, i32 %x, i32 %y) {
entry:
  br i1 %c, label %bb1, label %bb2
bb1:
  %a = trunc nuw nsw i32 %x to i16
  br label %end
bb2:
  %b = trunc nuw nsw i32 %y to i16
  br label %end
end:
  %p = phi i16 [ %a, %bb1 ], [ %b, %bb2 ]
  ret i16 %p
}
```
After: `%p = trunc i32 %p.in to i16`  (missing `nuw nsw`)

## Risk / scope

Missed-optimization. Same severity as w326. The flags carry real propagation
value -- `trunc nsw` permits sext-of-trunc-of-x = sext(trunc(x)) simplification
chains, `zext nneg` lets downstream code reason about the sign bit.

## Fix sketch

```cpp
if (CastInst *FirstCI = dyn_cast<CastInst>(FirstInst)) {
  CastInst *NewCI = CastInst::Create(FirstCI->getOpcode(), PhiVal, PN.getType());
  NewCI->copyIRFlags(PN.getIncomingValue(0));
  for (Value *V : drop_begin(PN.incoming_values()))
    NewCI->andIRFlags(V);
  PHIArgMergedDebugLoc(NewCI, PN);
  return NewCI;
}
```
