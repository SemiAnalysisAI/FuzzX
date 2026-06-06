# 014 - `exp2(log2(x))` folded with only outer-call `reassoc`

Component: `llvm/lib/Analysis/InstructionSimplify.cpp:6577`

The fold `exp2(log2(X)) -> X` checks only the outer call's `reassoc` flag. The
inner `log2` call may be fully strict libm semantics.

For `x = -1.0`, the source returns NaN. The folded target returns `-1.0`.
Because there is no `nnan`, the source NaN is not poison.

Verifier: Newton (019e98ff-7b9c-7740-bfd2-fd3fab56958e) returned YES.
