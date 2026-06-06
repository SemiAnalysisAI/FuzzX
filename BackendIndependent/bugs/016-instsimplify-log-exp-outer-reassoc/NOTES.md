# 016 - `log(exp(x))` folded with only outer-call `reassoc`

Component: `llvm/lib/Analysis/InstructionSimplify.cpp:6589`

The fold `log(exp(X)) -> X` uses only the outer `log` call's `reassoc` flag.
The inner `exp` call can be unflagged.

For `x = 1000.0`, `exp(1000.0)` overflows to `+inf`, and `log(+inf)` is
`+inf`. The optimized function returns finite `1000.0`. There is no `ninf`
flag to make the infinity poison.

Verifier: McClintock (019e98ff-c201-7783-acbf-7a9e4b1dede9) returned YES.
