# w338: SimpleLoopUnswitch trivial switch unswitch zeroes the default-case weight when the original switch has `!"expected"` `branch_weights`

Pass: `simple-loop-unswitch` (trivial path, runs in default x86 -O2)
File: `llvm/lib/Transforms/Scalar/SimpleLoopUnswitch.cpp` (manifestation)
Underlying: `llvm/lib/IR/Instructions.cpp` (`SwitchInstProfUpdateWrapper::getSuccessorWeight` static overload)

## Root cause

`unswitchTrivialSwitch` reads per-case weights from the original switch via the static overload `SwitchInstProfUpdateWrapper::getSuccessorWeight(const SwitchInst &SI, unsigned idx)`:

```cpp
// SimpleLoopUnswitch.cpp:840-841
SwitchInstProfUpdateWrapper::CaseWeightOpt DefaultCaseWeight =
    SwitchInstProfUpdateWrapper::getSuccessorWeight(SI, 0);
...
// SimpleLoopUnswitch.cpp:894-896
auto W = SIW.getSuccessorWeight(CaseI->getSuccessorIndex());
ExitCases.emplace_back(CaseI->getCaseValue(), CaseI->getCaseSuccessor(), W);
```

The static overload (Instructions.cpp:4248-4258) checks for the legacy 1-string-prefix layout only:

```cpp
SwitchInstProfUpdateWrapper::CaseWeightOpt
SwitchInstProfUpdateWrapper::getSuccessorWeight(const SwitchInst &SI,
                                                unsigned idx) {
  if (MDNode *ProfileData = getBranchWeightMDNode(SI))
    if (ProfileData->getNumOperands() == SI.getNumSuccessors() + 1)
      return mdconst::extract<ConstantInt>(ProfileData->getOperand(idx + 1))
          ->getValue()
          .getZExtValue();
  return std::nullopt;
}
```

When the input metadata carries the `"expected"` tag, `branch_weights` has `NumSuccessors + 2` operands (`"branch_weights", "expected", w0, w1, ...`), so this `if` is false and the method returns `std::nullopt`. The rest of `unswitchTrivialSwitch` then proceeds as if there were no weights:

- `DefaultCaseWeight = std::nullopt` → the `else if (DefaultCaseWeight)` branch at line 1005-1015 (where `SW = default + Σ case_weights` is computed and written back via `NewSIW.setSuccessorWeight(0, SW)`) is **skipped**.
- Each per-case `W` saved in `ExitCases` is also `std::nullopt`, so the `NewSIW.addCase(CaseVal, UnswitchedBB, W)` call (line 991) goes through `SwitchInstProfUpdateWrapper::addCase` with `W.value_or(0)`-style handling.

The result, observed on the cloned/hoisted switch: weight vector `[0, w_unswitched_case_1, w_unswitched_case_2]`, with the default zeroed. That is consistent with `addCase` (Instructions.cpp:4197-4213) — when called the first time with a non-null weight, it initializes `Weights = SmallVector(N, 0)` and then writes the single new slot, leaving the default at 0.

## Reproducer

```llvm
; opt -passes='simple-loop-unswitch<no-nontrivial;trivial>' -S

define void @f(ptr %p, i32 %n, i32 %c) {
entry:
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %inc, %backedge ]
  switch i32 %c, label %backedge [
    i32 1, label %exit1
    i32 2, label %exit2
  ], !prof !0

exit1:
  ret void
exit2:
  ret void

backedge:
  %inc = add i32 %i, 1
  %cmp = icmp slt i32 %inc, %n
  br i1 %cmp, label %loop, label %exit

exit:
  ret void
}
!0 = !{!"branch_weights", !"expected", i32 100, i32 1, i32 1}
```

## Diff

Before: `!0 = !{!"branch_weights", !"expected", i32 100, i32 1, i32 1}`  
  default(backedge)=100, case1=1, case2=1
After (hoisted unswitched switch in `entry`): `!0 = !{!"branch_weights", i32 0, i32 1, i32 1}`  
  default=**0**, case1=1, case2=1, and the `"expected"` origin is lost.

For comparison, the same input **without** `"expected"` round-trips cleanly:
- Before: `!0 = !{!"branch_weights", i32 100, i32 1, i32 1}`
- After:  `!0 = !{!"branch_weights", i32 100, i32 1, i32 1}` (preserved)

## Impact / why it matters in -O2

- The trivial unswitch path is active at default -O2.
- For switches built from `__builtin_expect` / `llvm.expect` (the metadata's `"expected"` origin), the hoisted switch ends up with a zero default-case weight and altered totals. Downstream `BranchProbabilityInfo` and codegen layout will treat the in-loop path (backedge) as never taken, biasing block layout / register allocation toward the rare exit cases and often inverting hot/cold for the loop body.
- Additionally, the `"expected"` origin tag is dropped, so passes that distinguish user-asserted hints from sampled estimates lose the distinction (same class as w337).

## Suggested fix

In `SwitchInstProfUpdateWrapper::getSuccessorWeight(const SwitchInst &, unsigned)`, replace the hard-coded `+ 1` with the existing `getBranchWeightOffset` helper:

```cpp
unsigned Off = getBranchWeightOffset(ProfileData);
if (ProfileData->getNumOperands() == SI.getNumSuccessors() + Off)
  return mdconst::extract<ConstantInt>(ProfileData->getOperand(idx + Off))
      ->getValue().getZExtValue();
```

`getBranchWeightOffset` is already used by `getNumBranchWeights` and the instance-level `init()`, so this brings the static overload in line with the rest of the API.
