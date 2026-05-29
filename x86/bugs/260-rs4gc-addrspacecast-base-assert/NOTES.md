# 260 — RewriteStatepointsForGC asserts "unsupported addrspacecast" (crash on valid IR)

Component: `llvm/lib/Transforms/Scalar/RewriteStatepointsForGC.cpp`
`findBaseDefiningValue`, CastInst arm (~line 502).

`rewrite-statepoints-for-gc` aborts on a one-way `addrspacecast` from a non-GC
`addrspace(0)` pointer into the GC `addrspace(1)`: `stripPointerCasts()` stops at
the addrspace(0) source, the address spaces differ, and the
"unsupported addrspacecast" assertion (added 2015, commit 8050a497) fires. Input
IR passes `-passes=verify`.

## Crash (HEAD 023e7decf625, assertions on)
```
Assertion failed: (... "unsupported addrspacecast"), function findBaseDefiningValue,
RewriteStatepointsForGC.cpp, line 502.
```
Distinct from the SUPPORTED round-trip `addrspace(1)->0->1` (where
stripPointerCasts walks back to the original GC base).

## Status
Crash-on-valid-IR. **Already filed upstream as open issue
[#61917](https://github.com/llvm/llvm-project/issues/61917)** (Apr 2023, unfixed
at HEAD) — confirmed still reproducing; not novel. Recorded for completeness.
