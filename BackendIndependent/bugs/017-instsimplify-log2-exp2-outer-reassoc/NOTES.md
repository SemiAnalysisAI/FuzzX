# 017 - `log2(exp2(x))` folded with only outer-call `reassoc`

Component: `llvm/lib/Analysis/InstructionSimplify.cpp:6595`

The fold `log2(exp2(X)) -> X` checks the outer call's FMF but not the inner
`exp2` call's flags.

For `x = 2000.0`, the source computes `exp2(2000.0) = +inf`, then
`log2(+inf) = +inf`. The folded target returns `2000.0`.

Verifier: Noether (019e9900-40de-7d30-a4b3-7f1b28af0b01) returned YES.
