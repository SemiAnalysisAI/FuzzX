# 005 - `pow(x, y) * x` folded through an unflagged `pow`

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:905`

The fold `pow(X, Y) * X -> pow(X, Y + 1)` runs from `foldFMulReassoc`, which is
entered when the outer multiply has `reassoc`. The matched `llvm.pow` call does
not need to carry `reassoc` or `afn`.

LangRef says rewrite-based flags such as `reassoc` must be present on all
instructions in a multi-instruction expression. Without `afn`, `llvm.pow`
returns the corresponding libm value.

For `x = +0.0, y = -1.0`, the source is `pow(+0.0, -1.0) * +0.0`, or
`+inf * +0.0`, which is NaN. The target computes `pow(+0.0, 0.0) = 1.0`.

Verifier: Huygens (019e98e4-652c-7603-be10-a7cb23683e98) returned YES.
