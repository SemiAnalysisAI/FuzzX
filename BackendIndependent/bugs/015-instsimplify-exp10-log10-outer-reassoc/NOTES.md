# 015 - `exp10(log10(x))` folded with only outer-call `reassoc`

Component: `llvm/lib/Analysis/InstructionSimplify.cpp:6583`

The fold `exp10(log10(X)) -> X` checks only the outer call's `reassoc` flag.
The unflagged `log10` call still has libm semantics.

For `x = -1.0`, the source returns NaN and the optimized result returns `-1.0`.

Verifier: Fermat (019e98ff-9c02-73e0-95ce-086c5908923c) returned YES.
