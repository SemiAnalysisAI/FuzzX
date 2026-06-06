# 018 - `log2(pow(2.0, x))` folded with only outer-call `reassoc`

Component: `llvm/lib/Analysis/InstructionSimplify.cpp:6595`

The `log2` simplifier also folds `log2(pow(2.0, X)) -> X` when only the outer
`log2` call has `reassoc`. The inner `pow` call is unflagged.

For `x = 2000.0`, `pow(2.0, 2000.0)` overflows to `+inf`, so the source returns
`log2(+inf) = +inf`. The optimized function returns finite `2000.0`.

Verifier: Feynman (019e9903-100f-7143-98fd-8f76d102d4ad) returned YES.
