# 004 - `tan(x) * cos(x)` folded to `sin(x)` without `afn`

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:1080`

InstCombine folds `tan(X) * cos(X)` to `sin(X)` when the outer multiply has
`contract`. LangRef says `contract` permits floating-point contraction such as
fusing multiply-adds; it does not permit replacing one set of libm intrinsic
calls with another. The libm-intrinsic approximation permission is `afn`.

For `x = 10.0`, the source `tan(x) * cos(x)` and target `sin(x)` differ by one
ulp under ordinary libm semantics. There is no `afn`, `nnan`, or other flag that
makes that value change poison or approximate.

Verifier: Meitner (019e98e4-64cc-7d00-9770-63d7b9267ea1) returned YES.
