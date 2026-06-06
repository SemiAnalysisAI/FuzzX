# 009 - `exp2(x) * exp2(y)` folded through unflagged `exp2` calls

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:942`

The fold `exp2(X) * exp2(Y) -> exp2(X + Y)` is performed based on only the
outer multiply's `reassoc`. The participating `exp2` calls are unflagged.

For `x = 2000.0, y = -2000.0`, the source computes `+inf * +0.0`, which is NaN.
The target computes `exp2(0.0) = 1.0`.

Verifier: Ampere (019e98e7-acbc-7a60-8686-cb72de7cf7ed) returned YES.
