# 013 - `exp(log(x))` folded with only outer-call `reassoc`

Component: `llvm/lib/Analysis/InstructionSimplify.cpp:6571`

InstructionSimplify folds `exp(log(X))` to `X` if the outer `exp` call has
`reassoc`. The inner `log` call is matched without checking its flags.

LangRef says rewrite-based flags such as `reassoc` must be present on every
instruction participating in a multi-instruction rewrite. Here only the outer
call is flagged.

For `x = -1.0`, unflagged `log(-1.0)` returns NaN under libm semantics, and
`exp(NaN)` returns NaN. The transformed function returns `-1.0`.

Verifier: Darwin (019e98ff-5550-70b3-a75d-9573f92e6754) returned YES.
