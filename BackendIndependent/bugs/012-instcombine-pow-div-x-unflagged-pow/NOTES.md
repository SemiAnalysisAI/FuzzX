# 012 - `pow(x, y) / x` folded through an unflagged `pow`

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:2260`

InstCombine folds `pow(X, Y) / X` to `pow(X, Y - 1)` when the outer divide has
`reassoc`. The original `pow` call may be unflagged.

For `x = +0.0, y = +1.0`, the source computes `pow(+0.0, +1.0) / +0.0`, or
`+0.0 / +0.0`, which is NaN. The target computes `pow(+0.0, 0.0) = 1.0`.

LangRef says multi-instruction rewrite-based transforms require all
participating instructions to carry the needed flag. The original `pow` call
does not.

Verifier: Parfit (019e98fb-65ec-7ec0-98d4-801227e1a99a) returned YES.
