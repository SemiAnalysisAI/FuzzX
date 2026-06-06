# 008 - `exp(x) * exp(y)` folded through unflagged `exp` calls

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:934`

The fold `exp(X) * exp(Y) -> exp(X + Y)` is guarded by `reassoc` on the outer
multiply, but the two `llvm.exp` calls are unflagged. LangRef requires
rewrite-based flags to be present on all participating instructions.

For `x = 1000.0, y = -1000.0`, ordinary libm semantics give
`exp(1000.0) = +inf` and `exp(-1000.0) = +0.0`, so the source multiply returns
NaN. The target computes `exp(0.0) = 1.0`.

Verifier: Ramanujan (019e98e7-ac22-77c1-a87f-3435519bdc26) returned YES.
