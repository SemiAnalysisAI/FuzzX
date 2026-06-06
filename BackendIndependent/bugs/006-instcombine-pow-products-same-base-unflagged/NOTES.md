# 006 - `pow(x, y) * pow(x, z)` folded through unflagged `pow` calls

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:918`

The fold `pow(X, Y) * pow(X, Z) -> pow(X, Y + Z)` is guarded by `reassoc` on the
outer multiply but does not require the two `pow` calls to carry any rewrite
flag.

For `x = +0.0, y = 1.0, z = -1.0`, the source computes
`pow(+0.0, 1.0) * pow(+0.0, -1.0)`, or `+0.0 * +inf`, which is NaN. The target
computes `pow(+0.0, 0.0) = 1.0`.

LangRef requires all participating instructions in a multi-instruction
rewrite to have the needed rewrite-based flag. The inner `pow` calls are
unflagged and have libm semantics.

Verifier: Euclid (019e98e4-6600-7d32-bf23-82765110ef57) returned YES.
