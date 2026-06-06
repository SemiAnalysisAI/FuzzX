# 007 - `pow(x, y) * pow(z, y)` folded through unflagged `pow` calls

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:926`

The fold `pow(X, Y) * pow(Z, Y) -> pow(X * Z, Y)` is performed based on the
outer multiply's `reassoc` flag. The two libm `pow` calls are matched without
requiring rewrite flags.

For `x = -1.0, z = -1.0, y = 0.5`, the source computes
`pow(-1.0, 0.5) * pow(-1.0, 0.5)`, which is `NaN * NaN`. The target computes
`pow(1.0, 0.5) = 1.0`.

Verifier: Bacon (019e98e7-abc0-7912-9f0c-ac9ee98e5620) returned YES.
