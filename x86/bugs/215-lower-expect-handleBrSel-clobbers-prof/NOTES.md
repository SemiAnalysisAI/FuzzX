# 215 — LowerExpectIntrinsic `handleBrSelExpect` unconditionally clobbers existing `!prof`

Component: `llvm/lib/Transforms/Scalar/LowerExpectIntrinsic.cpp` line ~348

When `llvm.expect` reaches a branch with pre-existing PGO `!prof` metadata (e.g., `{branch_weights, 5000, 100}`), the lowering unconditionally calls `BSI.setMetadata(MD_prof, Node)` and replaces the measured weights with the expect-style weights `{1, 2000}`. Real profile data is destroyed in favor of a static hint.

## Reproducer

`opt -passes=lower-expect -S repro.ll` — input `!prof !{branch_weights, 5000, 100}` becomes output `!prof !{branch_weights, "expected", 1, 2000}`. The "expected" tag flips the direction of the weights AND replaces the measured magnitudes.

## Severity

Default x86 -O2 when source uses `__builtin_expect` AND has PGO. Wrong profile data propagates through MachineBlockPlacement, regalloc hot-spilling decisions, and inline-cost computation.

## Fix

Use `hasBranchWeightMD()` to detect pre-existing PGO weights and either preserve them or merge with the expect-style weights (e.g., scale by measured magnitudes).
