# 021 - `z / exp(y)` folded through an unflagged `exp`

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:1991`

`foldFDivPowDivisor` rewrites `z / exp(y)` to `z * exp(-y)` when the outer
`fdiv` has `reassoc arcp`. The original `exp` call may be unflagged.

`arcp` licenses treating division as multiplication by a reciprocal, but the
transform also replaces the reciprocal of a precise libm `exp(y)` with a new
`exp(-y)` call. LangRef requires rewrite-based flags on all participating
instructions in a multi-instruction rewrite; the original `exp` has none.

For `z = 1.0, y = 0.7`, host libm gives source `1.0 / exp(0.7)` bits
`0x3fdfc80db9dd5541`, while the target `exp(-0.7)` has bits
`0x3fdfc80db9dd5542`.

Verifier: Archimedes (019e990c-3409-70b0-b649-4a1fee4dcacc) returned YES.
