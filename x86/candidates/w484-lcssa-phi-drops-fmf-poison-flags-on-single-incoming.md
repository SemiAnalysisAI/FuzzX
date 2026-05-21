# LCSSA-inserted exit PHI drops FMF (poison-generating `nnan`/`ninf`) from its single incoming value

## File and root cause

`llvm/lib/Transforms/Utils/LCSSA.cpp:163-170` in `formLCSSAForInstructionsImpl`.

```c++
PHINode *PN = PHINode::Create(I->getType(), PredCache.size(ExitBB),
                              I->getName() + ".lcssa");
PN->insertBefore(ExitBB->begin());
if (InsertedPHIs)
  InsertedPHIs->push_back(PN);
// Get the debug location from the original instruction.
PN->setDebugLoc(I->getDebugLoc());

// Add inputs from inside the loop for this PHI. This is valid
// because `I` dominates `ExitBB` (checked above).
for (BasicBlock *Pred : PredCache.get(ExitBB)) {
  PN->addIncoming(I, Pred);
  ...
}
```

`PHINode::Create` produces a `phi` with **no fast-math flags** even for an FP
type. Per `llvm/include/llvm/IR/Operator.h:349-380`, `FPMathOperator::classof`
returns true for `Instruction::PHI` with a "supported floating-point type",
so an FP-typed phi *does* carry FMF (`SubclassOptionalData` of the phi). The
LCSSA insertion code never calls `PN->copyFastMathFlags(I)` or similar, even
though `I` is the unique source value flowing into this phi from inside the
loop.

`Operator::hasPoisonGeneratingFlags()` (`llvm/lib/IR/Operator.cpp:67-70`)
identifies **`nnan` and `ninf`** as poison-generating on any FPMathOperator,
including phi:

```c++
default:
  if (const auto *FP = dyn_cast<FPMathOperator>(this))
    return FP->hasNoNaNs() || FP->hasNoInfs();
  return false;
```

So the LCSSA phi created for a value that carried `nnan`/`ninf` is a strictly
weaker poison-shape than the source instruction. Downstream consumers
(InstCombine, LoopVectorize reduction recognition, SLPVectorize, SCEV-based
loop pipelines that re-derive recurrence FMF) read FMF from the phi to decide
on reassociation or NaN/Inf canonicalization. Without the flag on the LCSSA
phi, those passes pessimize.

## Reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

define float @f(i32 %n, float %s) {
entry:
  br label %h

h:
  %i = phi i32 [0, %entry], [%inc, %h]
  %acc = phi float [%s, %entry], [%y, %h]
  %f = sitofp i32 %i to float
  %y = fmul nnan ninf nsz reassoc float %acc, %f
  %inc = add i32 %i, 1
  %c = icmp slt i32 %i, %n
  br i1 %c, label %h, label %exit

exit:
  %r = fadd float %y, 0.000000e+00
  ret float %r
}
```

### `opt -passes='lcssa' -S` actual output

```llvm
exit:                                             ; preds = %h
  %y.lcssa = phi float [ %y, %h ]    ; <-- no FMF flags whatsoever
  %r = fadd float %y.lcssa, 0.000000e+00
  ret float %r
```

The single-incoming `%y.lcssa` phi has none of the `nnan ninf nsz reassoc`
flags that were on `%y`. Because there is only one incoming, `%y.lcssa` is
semantically `%y` — adding the same FMF to it would be sound and would
unblock downstream FMF-gated transforms.

## Why this is a regression (and why it qualifies as "drops poison-generating flags")

* In LLVM's IR semantics, `nnan` and `ninf` on a value generate **poison** if
  the value is NaN/Inf, respectively. The LCSSA phi-of-one ought to be a
  pass-through of that poison-generation property; instead it is the weaker
  "no flags = no poison generation, only the actual value".
* For passes that look at the phi (e.g., InstCombine's `visitPHINode`,
  LoopVectorize's `isFMFContractRecurrence`, IVDescriptors `RecurrenceDescriptor`)
  the difference between "phi has `nnan`" and "phi has no FMF" controls
  whether the recurrence can be considered finite and reassociation-safe.
* LCSSA is part of the canonicalization run before LoopVectorize, LoopUnroll,
  and LoopUnswitch in `-O2`, so this drop affects the entire loop pipeline
  for FMF-heavy FP code (HPC, ML kernels).
* The mirror bug exists for *integer* poison flags (`nsw`, `nuw`, `nneg`,
  `samesign`, `disjoint`, `exact`) — but `PHINode` does not carry those for
  integer ops (they belong only to specific opcodes). For integer LCSSA the
  user instructions outside the loop need to be conservative about poison
  anyway. So the integer side is "by design"; the FP side leaks
  optimization-relevant information.

## Fix sketch

In the single-incoming case (the common LCSSA outcome), copy FMF from the
sole incoming instruction:

```c++
PHINode *PN = PHINode::Create(I->getType(), PredCache.size(ExitBB),
                              I->getName() + ".lcssa");
PN->insertBefore(ExitBB->begin());
PN->setDebugLoc(I->getDebugLoc());

// New: if this is an FP phi and I is the only contributor, share FMF.
if (isa<FPMathOperator>(PN))
  if (auto *FPI = dyn_cast<FPMathOperator>(I))
    PN->copyFastMathFlags(I);
```

For the multi-incoming case the safe operation is intersection of FMF across
all incoming definitions; but here LCSSA's pattern is exactly "one I across
all preds", so direct copy is sound.

Related: `SSAUpdater::RewriteUse` (used at `LCSSA.cpp:242` for non-exit-block
uses) suffers from the same lacuna for any phi nodes it inserts in
intermediate join blocks within the loop.
