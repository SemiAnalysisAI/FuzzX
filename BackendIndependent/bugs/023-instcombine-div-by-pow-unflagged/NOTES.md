# 023 - `z / pow(x, y)` folded through an unflagged `pow`

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:1991`

`foldFDivPowDivisor` rewrites `z / pow(x, y)` to `z * pow(x, -y)` when the
outer divide has `reassoc arcp`. The original `pow` call may be unflagged, so
it has precise libm semantics rather than approximate-function semantics.

For `z = 1.0, x = 0.1, y = 0.2`, host libm gives source
`1.0 / pow(0.1, 0.2)` bits `0x3ff95bb8f6d46052`, while the target
`pow(0.1, -0.2)` has bits `0x3ff95bb8f6d46053`.

Verifier: Poincare (019e990c-931f-7ae2-9816-ebc327398bbe) returned YES.
