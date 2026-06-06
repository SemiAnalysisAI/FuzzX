# 011 - `cos(x) / sin(x)` folded to `1.0 / tan(x)` without `afn`

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:2208`

This is the cotangent sibling of #010. InstCombine replaces `cos(X) / sin(X)`
with `1.0 / tan(X)` based only on `reassoc` on the outer divide.

For `x = 10.0`, libm `cos(x) / sin(x)` and `1.0 / tan(x)` differ by one ulp.
The original `sin` and `cos` calls are unflagged libm intrinsics, so replacing
them with `tan` is not licensed by only the outer arithmetic instruction.

Verifier: James (019e98f7-7ed6-7bd2-955f-6bb830925a65) returned YES.
