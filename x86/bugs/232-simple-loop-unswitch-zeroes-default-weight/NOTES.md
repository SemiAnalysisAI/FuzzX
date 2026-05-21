# 232 — SimpleLoopUnswitch trivial switch unswitch zeroes default-case branch weight when source switch has `"expected"` tag

Component: `llvm/lib/Transforms/Scalar/SimpleLoopUnswitch.cpp` `unswitchTrivialSwitch` lines ~840-841 and ~894-896, root cause in `llvm/lib/IR/Instructions.cpp` `SwitchInstProfUpdateWrapper::getSuccessorWeight` lines ~4248-4258.

`getSuccessorWeight(const SwitchInst&, unsigned)` hard-codes a `+1` offset for the operand layout, which fails for switches whose `!prof` carries the `"expected"` origin tag (N+2 operands). It returns `std::nullopt`, and `unswitchTrivialSwitch` then writes a default-case weight of 0 via `addCase` zero-init.

## Reproducer

`opt -O2 -S repro.ll`

Input switch has `!prof !{branch_weights, "expected", 100, 1, 1}` (default=100, c0=1, c1=1). Output: `!prof !{branch_weights, 0, 1, 1}` — default weight zeroed.

## Severity

Default x86 -O2. PGO with `__builtin_expect`-style tags get corrupted across loop-unswitch — branch placement and code layout are subsequently wrong.

## Fix

Update `getSuccessorWeight` to handle both layouts (with/without `"expected"`), or use `hasBranchWeightOrigin` and adjust offset accordingly.
