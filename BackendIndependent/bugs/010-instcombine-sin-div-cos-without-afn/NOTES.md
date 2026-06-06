# 010 - `sin(x) / cos(x)` folded to `tan(x)` without `afn`

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:2208`

InstCombine folds `sin(X) / cos(X)` to `tan(X)` when the outer divide has
`reassoc`. The `sin` and `cos` calls do not need to carry `reassoc` or `afn`.

Without `afn`, these intrinsics return the corresponding libm values. For
`x = 10.0`, libm `sin(x) / cos(x)` and libm `tan(x)` differ by one ulp. The
outer `reassoc` does not make the inner libm calls approximate and does not
exclude NaNs or rounding differences.

Verifier: Bernoulli (019e98f7-5164-7741-94d3-1006ad00f0b6) returned YES.
