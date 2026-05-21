# w326: foldPHIArgBinOpIntoPHI's CmpInst path drops IR flags entirely (two sites)

## Summary

`InstCombinerImpl::foldPHIArgBinOpIntoPHI` (and the sibling fold
`foldPHIArgOpIntoPHI` for the constant-RHS case) handle two final shapes for
the fused operation:

- BinaryOperator (lines 524-534 / 960-969): `copyIRFlags(IncomingValue(0))`
  then `andIRFlags(V)` for the rest. This intersects nsw/nuw/exact/disjoint/
  nneg/fmf etc across all incoming binops.
- CmpInst (lines 517-522 / 971-975): plain
  `CmpInst::Create(opcode, predicate, ...)`. **No `copyIRFlags`/`andIRFlags`
  call at all.**

The result: any flag that an ICmp/FCmp can carry -- `samesign` on ICmp, FMF
(`nnan`/`ninf`/`nsz`/`arcp`/`contract`/`afn`/`reassoc`) on FCmp -- is silently
dropped from the merged compare even when **every** incoming compare had it.

This is the inverse-asymmetry of the BinaryOperator path. Since the new compare
starts with empty flags and never picks them up, the direction is always
"safer" (we drop knowledge). That makes this a missed-optimization, not a
miscompile. But it is a clearly inconsistent fold versus the BinOp arm and is
worth fixing for parity.

## Source

`llvm/lib/Transforms/InstCombine/InstCombinePHI.cpp`:

Site A -- `foldPHIArgBinOpIntoPHI` (non-constant RHS path):
```
517    if (CmpInst *CIOp = dyn_cast<CmpInst>(FirstInst)) {
518      CmpInst *NewCI = CmpInst::Create(CIOp->getOpcode(), CIOp->getPredicate(),
519                                       LHSVal, RHSVal);
520      PHIArgMergedDebugLoc(NewCI, PN);
521      return NewCI;                          ; <-- no copy/andIRFlags
522    }
523
524    BinaryOperator *BinOp = cast<BinaryOperator>(FirstInst);
525    BinaryOperator *NewBinOp =
526      BinaryOperator::Create(BinOp->getOpcode(), LHSVal, RHSVal);
527
528    NewBinOp->copyIRFlags(PN.getIncomingValue(0));
529
530    for (Value *V : drop_begin(PN.incoming_values()))
531      NewBinOp->andIRFlags(V);
```

Site B -- `foldPHIArgOpIntoPHI` (constant RHS path):
```
960    if (BinaryOperator *BinOp = dyn_cast<BinaryOperator>(FirstInst)) {
961      BinOp = BinaryOperator::Create(BinOp->getOpcode(), PhiVal, ConstantOp);
962      BinOp->copyIRFlags(PN.getIncomingValue(0));
963      for (Value *V : drop_begin(PN.incoming_values()))
964        BinOp->andIRFlags(V);
965      PHIArgMergedDebugLoc(BinOp, PN);
966      return BinOp;
967    }
968
969    ; <-- BinOp branch above does copy/and; CmpInst branch below skips.
971    CmpInst *CIOp = cast<CmpInst>(FirstInst);
972    CmpInst *NewCI = CmpInst::Create(CIOp->getOpcode(), CIOp->getPredicate(),
973                                     PhiVal, ConstantOp);
974    PHIArgMergedDebugLoc(NewCI, PN);
975    return NewCI;                            ; <-- no copy/andIRFlags
```

`andIRFlags` (`llvm/lib/IR/Instruction.cpp:805-807`) correctly handles
`SrcICmp->hasSameSign()` for ICmps, and `FastMathFlags` for FPMathOperators
(line 788-794), so the helper is already capable. The PHI fold just never
invokes it on the CmpInst path in either location.

## Reproducer

```llvm
target datalayout = "e-m:e-p:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i1 @test_cmp(i1 %c, i32 %x, i32 %y, i32 %k) {
entry:
  br i1 %c, label %bb1, label %bb2
bb1:
  %a = icmp samesign ult i32 %x, %k
  br label %end
bb2:
  %b = icmp samesign ult i32 %y, %k
  br label %end
end:
  %p = phi i1 [ %a, %bb1 ], [ %b, %bb2 ]
  ret i1 %p
}
```

## Diff: `opt -passes=instcombine -S`

Before:
```
bb1:  %a = icmp samesign ult i32 %x, %k
bb2:  %b = icmp samesign ult i32 %y, %k
end:  %p = phi i1 [ %a, %bb1 ], [ %b, %bb2 ]
```

After (BUG, missed-opt: samesign gone even though BOTH incomings had it):
```
end:
  %x.pn = phi i32 [ %x, %bb1 ], [ %y, %bb2 ]
  %p = icmp ult i32 %x.pn, %k                  ; <-- no `samesign`
  ret i1 %p
```

Expected:
```
  %p = icmp samesign ult i32 %x.pn, %k
```

### FMF drop on FCmp (constant RHS path, site B):
```llvm
define i1 @test(i1 %c, float %x, float %y) {
entry:
  br i1 %c, label %bb1, label %bb2
bb1:
  %a = fcmp fast olt float %x, 1.0
  br label %end
bb2:
  %b = fcmp fast olt float %y, 1.0
  br label %end
end:
  %p = phi i1 [ %a, %bb1 ], [ %b, %bb2 ]
  ret i1 %p
}
```
After: `%p = fcmp olt float %p.in, 1.000000e+00`  (missing `fast`)

## Risk / scope

Missed-optimization on every PHI-of-compare-with-flags pattern. ICmp `samesign`
is increasingly used to enable downstream specialization; FCmp FMF is widely
used in numerical code. Easy mechanical fix:

```cpp
if (CmpInst *CIOp = dyn_cast<CmpInst>(FirstInst)) {
  CmpInst *NewCI = CmpInst::Create(...);
  NewCI->copyIRFlags(PN.getIncomingValue(0));
  for (Value *V : drop_begin(PN.incoming_values()))
    NewCI->andIRFlags(V);
  PHIArgMergedDebugLoc(NewCI, PN);
  return NewCI;
}
```
