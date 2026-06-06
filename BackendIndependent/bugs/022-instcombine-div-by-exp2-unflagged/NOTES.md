# 022 - `z / exp2(y)` folded through an unflagged `exp2`

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:1991`

`foldFDivPowDivisor` rewrites `z / exp2(y)` to `z * exp2(-y)` based only on
the outer `fdiv reassoc arcp`. The original `exp2` call is unflagged and has
precise libm semantics.

For `z = 1.0, y = 0.3`, host libm gives source `1.0 / exp2(0.3)` bits
`0x3fe9fdf8bcce533d`, while the target `exp2(-0.3)` has bits
`0x3fe9fdf8bcce533e`.

Verifier: Volta (019e990c-5f38-7fd2-9eb3-74a78cb419f1) returned YES.
