# 020 - `log10(pow(10.0, x))` folded with only outer-call `reassoc`

Component: `llvm/lib/Analysis/InstructionSimplify.cpp:6603`

The `log10` simplifier also folds `log10(pow(10.0, X)) -> X` based only on the
outer `log10` call's `reassoc` flag. The inner `pow` call is unflagged.

For `x = 400.0`, `pow(10.0, 400.0)` overflows to `+inf`, so the source returns
`log10(+inf) = +inf`. The optimized function returns finite `400.0`.

Verifier: Herschel (019e9903-5fc0-7151-8382-448a7f259175) returned YES.
