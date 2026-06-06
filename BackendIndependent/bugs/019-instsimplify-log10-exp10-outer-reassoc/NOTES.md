# 019 - `log10(exp10(x))` folded with only outer-call `reassoc`

Component: `llvm/lib/Analysis/InstructionSimplify.cpp:6603`

The fold `log10(exp10(X)) -> X` checks only `reassoc` on the outer `log10`.
The inner `exp10` call is unflagged.

For `x = 400.0`, `exp10(400.0)` overflows to `+inf`, and `log10(+inf)` is
`+inf`. The optimized function returns `400.0`.

Verifier: Locke (019e9903-3ddd-7620-a3e6-88b7eeccfc89) returned YES.
