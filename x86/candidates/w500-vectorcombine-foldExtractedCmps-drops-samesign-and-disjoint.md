# w500 - VectorCombine `foldExtractedCmps` drops `samesign` on inner icmps and `disjoint` (etc.) on outer binop

## Location

`llvm/lib/Transforms/Vectorize/VectorCombine.cpp`

- Entry: `VectorCombine::foldExtractedCmps` line 1466
- Defective creates:
  - line 1553: `Value *VCmp = Builder.CreateCmp(Pred, X, ConstantVector::get(CmpC));`
  - line 1557: `Value *VecLogic = Builder.CreateBinOp(BI->getOpcode(), LHS, RHS);`

```cpp
// line 1506
CmpInst::Predicate Pred = *MatchingPred;
...
// line 1553 – Pred is CmpInst::Predicate (raw enum, NO samesign bit).
Value *VCmp = Builder.CreateCmp(Pred, X, ConstantVector::get(CmpC));
...
// line 1557 – new outer logic binop is created with default flags.
Value *VecLogic = Builder.CreateBinOp(BI->getOpcode(), LHS, RHS);
```

Neither the new vector compare nor the new vector logic op copies IR flags
from the two original `icmp` instructions or from the original outer
binop. Two flag classes are lost:

1. `samesign` on the original `icmp samesign` operands — `CmpPredicate` is
   stripped to `CmpInst::Predicate` on line 1506, so `CreateCmp` produces
   a plain `icmp` without `samesign`. (Compare with
   `VectorCombine::scalarizeOpOrCmp` at line 1456 which DOES call
   `ScalarInst->copyIRFlags(&I)` to preserve the same flag — proving the
   intended invariant.)
2. `disjoint` on `or`, `nuw`/`nsw` on `add/sub/mul`, `exact` on shifts —
   none of these apply directly to i1 logic ops in normal IR, but
   `disjoint` on `or i1` is meaningful and allowed.

For both flag classes, the result is a strict weakening: information that
the original IR carried is silently dropped, so subsequent passes
(InstSimplify, InstCombine, value-tracking-based folds) lose the
preconditions that justified producing them.

## Repro 1 — `samesign` is dropped

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define i1 @samesign_extcmps(<4 x i32> %x) {
  %e0 = extractelement <4 x i32> %x, i32 0
  %e1 = extractelement <4 x i32> %x, i32 1
  %c0 = icmp samesign slt i32 %e0, 5
  %c1 = icmp samesign slt i32 %e1, 7
  %r  = and i1 %c0, %c1
  ret i1 %r
}
```

```
opt -mtriple=x86_64-unknown-linux-gnu -passes=vector-combine -S
```

Output:

```llvm
define i1 @samesign_extcmps(<4 x i32> %x) {
  %1 = icmp slt <4 x i32> %x, <i32 5, i32 7, i32 poison, i32 poison>
  %shift = shufflevector <4 x i1> %1, <4 x i1> poison, <4 x i32> <i32 1, i32 poison, i32 poison, i32 poison>
  %2 = and <4 x i1> %1, %shift
  %r = extractelement <4 x i1> %2, i64 0
  ret i1 %r
}
```

The original two `icmp samesign slt` became a single `icmp slt <4 x i32>`
with the `samesign` flag gone.

## Repro 2 — `disjoint` is dropped

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define i1 @ec_disjoint_or(<4 x i32> %x) {
  %e0 = extractelement <4 x i32> %x, i32 0
  %e1 = extractelement <4 x i32> %x, i32 1
  %c0 = icmp slt i32 %e0, 5
  %c1 = icmp slt i32 %e1, 7
  %r  = or disjoint i1 %c0, %c1
  ret i1 %r
}
```

Output:

```llvm
define i1 @ec_disjoint_or(<4 x i32> %x) {
  %1 = icmp slt <4 x i32> %x, <i32 5, i32 7, i32 poison, i32 poison>
  %shift = shufflevector <4 x i1> %1, <4 x i1> poison, <4 x i32> <i32 1, i32 poison, i32 poison, i32 poison>
  %2 = or <4 x i1> %1, %shift          ; <-- `disjoint` gone
  %r = extractelement <4 x i1> %2, i64 0
  ret i1 %r
}
```

## Default x86 -O2 reproduces

```
opt -mtriple=x86_64-unknown-linux-gnu -O2 -S
```

produces

```llvm
%1 = shufflevector <4 x i32> %x, <4 x i32> poison, <2 x i32> <i32 0, i32 1>
%2 = icmp slt <2 x i32> %1, <i32 5, i32 7>     ; samesign gone
%shift = shufflevector <2 x i1> %2, <2 x i1> poison, <2 x i32> <i32 1, i32 poison>
%foldExtExtBinop = and <2 x i1> %2, %shift
%r = extractelement <2 x i1> %foldExtExtBinop, i64 0
```

i.e. the bug fires in the regular -O2 pipeline.

## Fix sketch

In `foldExtractedCmps`, mirror what `scalarizeOpOrCmp` already does:

```cpp
auto *VCmpI = dyn_cast<Instruction>(VCmp);
if (auto *I0Inst = dyn_cast<Instruction>(B0))
  if (auto *I1Inst = dyn_cast<Instruction>(B1))
    if (VCmpI) { VCmpI->copyIRFlags(I0Inst); VCmpI->andIRFlags(I1Inst); }

auto *VecLogicI = dyn_cast<Instruction>(VecLogic);
if (VecLogicI) VecLogicI->copyIRFlags(BI);   // preserve `disjoint`, etc.
```

This matches the convention used for the binop branch of
`foldExtractExtract` (line 638 — `VecBOInst->copyIRFlags(&I)`).
