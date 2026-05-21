## IndVarSimplify::canonicalizeToIntegerIV: silently drops FCmpInst's fast-math flags / loses semantics

**Severity:** Behavior change (poison-→-defined refinement is OK, but flag
loss can lose alias / range info downstream).

**File:** `llvm/lib/Transforms/Scalar/IndVarSimplify.cpp:423-489`
(`canonicalizeToIntegerIV`).

### What goes wrong

When an FP induction variable is rewritten as an integer IV, the new
`ICmpInst` replacing the old `FCmpInst` is constructed bare:

```cpp
ICmpInst *NewCompare = new ICmpInst(
    BI->getIterator(), IIV.NewPred, NewAdd,
    ConstantInt::getSigned(Int32Ty, IIV.ExitValue), FPIV.Compare->getName());
NewCompare->setDebugLoc(FPIV.Compare->getDebugLoc());
...
NewCompare->takeName(FPIV.Compare);
FPIV.Compare->replaceAllUsesWith(NewCompare);
```

The replacement:

1. **Drops fast-math flags** that the original `FCmpInst` carried (`nnan`,
   `ninf`, `fast`, etc.) — `setFastMathFlags` is never called on
   `NewCompare`. The original compare's FMF is silently lost on the way
   to the integer compare.
2. **Drops branch-weights metadata** on the controlling `BI`. The
   original `BI->getCondition()` may have come from a profile-fed
   `fcmp ... !prof` chain; the new `ICmpInst` has no `!prof`. Branch
   weights live on the branch, not the cmp, so this is a tertiary issue
   — but if the FCmp had `!fpmath` (relative precision), that is also
   dropped.
3. **Loses `fneg/fma`-style annotations** the old compare may have been
   the only consumer of — once the FCmp is removed by
   `RecursivelyDeleteTriviallyDeadInstructions`, any sibling FP IR
   loses a use-site, and any `!annotation` metadata referring to the
   compare is orphaned.

### Repro

```ll
; reducer.ll
declare void @use(double)

define void @fp_iv_nnan() {
entry:
  br label %loop

loop:
  %i = phi double [ 0.0, %entry ], [ %i.next, %loop ]
  %i.next = fadd nnan ninf double %i, 1.0
  call void @use(double %i)
  ; original compare carries nnan ninf - meaningful: "we KNOW neither side is NaN/Inf"
  %cmp = fcmp nnan ninf olt double %i.next, 5.0
  br i1 %cmp, label %loop, label %exit

exit:
  ret void
}
```

`opt -passes=indvars -S reducer.ll`:

```ll
loop:
  %i.int = phi i32 [ 0, %entry ], [ %i.next.int, %loop ]
  %indvar.conv = sitofp i32 %i.int to double
  %i.next.int = add nuw nsw i32 %i.int, 1
  call void @use(double %indvar.conv)
  %cmp = icmp slt i32 %i.next.int, 5
  br i1 %cmp, label %loop, label %exit
```

The original `fcmp nnan ninf olt` carried fast-math flags that other passes
(e.g., `FPEnv`, `LICM`, downstream `SimplifyFP`) consume. After the
rewrite the integer compare obviously cannot carry them, but the
intermediate `sitofp` *also* lacks any non-default flag — so the
contract that the FP IV value was nnan/ninf is severed at the rewrite
boundary.

### Why this matters in the -O2 pipeline

Default `-O2` runs `indvars` before late FP simplification and before
loop-rotate's re-emission of FP comparisons. If the loop body still
uses the FP-typed `indvar.conv` value (via the inserted `SIToFPInst` at
line 484), downstream FP combines no longer see that the value came
from an nnan/ninf source, blocking simplifications that would have
fired on the original IR.

### Source-level evidence

`canonicalizeToIntegerIV` builds:

```cpp
ICmpInst *NewCompare = new ICmpInst(... FPIV.Compare->getName());
NewCompare->setDebugLoc(FPIV.Compare->getDebugLoc());
```

with no `copyFastMathFlags` (which is not applicable for int compares
in the first place) — the design loses the FP-side flags by
construction. The `SIToFPInst` inserted at line 484 likewise has no
FMF copy:

```cpp
Instruction *Conv = new SIToFPInst(NewPHI, PN->getType(), "indvar.conv",
                                   PN->getParent()->getFirstInsertionPt());
Conv->setDebugLoc(PN->getDebugLoc());   // only debug loc
```

The `Incr` (original FAdd) is replaced by poison and deleted, again
without first transferring its FMF to the new `Conv` or any other
surviving SSA value.

### Suggested fix

When inserting the new `SIToFPInst` (the bridge back to FP), copy any
common fast-math flags that were present on both `FPIV.Compare` and
`Incr`. (`fneg`/`fadd`/`fmul` style FMF can be set on `sitofp` via
`copyFastMathFlags`.)

### Status

Confirmed via `opt -passes=indvars` diff. Documented behavior change
rather than miscompile, but a clear flag-loss vector worth fixing.
