# 216 — LowerExpectIntrinsic `handleSwitchExpect` unconditionally clobbers existing per-case `!prof`

Component: `llvm/lib/Transforms/Scalar/LowerExpectIntrinsic.cpp` lines ~104-106

Sibling of #215 for switches. Input weights `{10, 500, 400}` become `{1, 2000, 1}` — per-case measured magnitudes are destroyed.

## Reproducer

`opt -passes=lower-expect -S repro.ll` — output `!prof = {branch_weights, "expected", 1, 2000, 1}` regardless of original.

## Severity

Default x86 -O2 when PGO is active. SwitchLowering decisions (jump-table vs cascade vs binary search) directly depend on per-case weights; clobbering them changes generated code.

## Fix

Same as #215: detect pre-existing measured weights and either preserve or merge.
