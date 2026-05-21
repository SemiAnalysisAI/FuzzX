# w42: LoopRotation.cpp and LoopUnrollPass.cpp are thin drivers, no in-file bugs

## Files
- `llvm/lib/Transforms/Scalar/LoopRotation.cpp` (107 lines)
- `llvm/lib/Transforms/Scalar/LoopUnrollPass.cpp` (1877 lines)

## Summary
- `LoopRotation.cpp` is just the pass-manager driver (`LoopRotatePass::run`)
  that delegates to `llvm::LoopRotation` in
  `lib/Transforms/Utils/LoopRotationUtils.cpp`. No latch-PHI handling lives in
  this file; the latch-PHI bug pattern in the task description would have to be
  searched in LoopRotationUtils.cpp.
- `LoopUnrollPass.cpp` makes the unroll cost/policy decisions and calls
  `UnrollLoop` (in `lib/Transforms/Utils/LoopUnroll.cpp`). The actual cloning
  loop body and metadata propagation (where the noalias-across-iterations
  pattern would live) is in `LoopUnroll.cpp`, NOT in LoopUnrollPass.cpp. No
  metadata copy / clone instructions exist in LoopUnrollPass.cpp.

## Patterns ruled out in these two files
- "LoopRotation that mishandles the latch PHI for a non-trivial header" — the
  rotation algorithm is not in `LoopRotation.cpp`; would need to be found in
  `LoopRotationUtils.cpp::LoopRotate::rotateLoop` (lines 349-...).
- "LoopUnroll that drops noalias metadata across iterations" — no cloning code
  in `LoopUnrollPass.cpp`; would need to be found in
  `Utils/LoopUnroll.cpp::UnrollLoop`.

Status: nothing filed from these two driver files. Recommend re-scoping to the
Utils files if these patterns are still wanted.
