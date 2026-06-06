# 041 - active lane mask constant fold uses wrapping arithmetic

Component: `llvm/lib/Analysis/ConstantFolding.cpp:4343`

LangRef defines `llvm.get.active.lane.mask` as:

```llvm
%m[i] = icmp ult (%base + i), %n
```

and states that overflow cannot occur because the addition and comparison are
performed in mathematical integers, not machine integers.

The constant folder converts both operands to `uint64_t` and evaluates
`Base + i < Limit`, so the addition wraps. In this reproducer, `base` is
`2^64 - 2` and `limit` is `2`. The mathematical lane values are:

```text
2^64 - 2, 2^64 - 1, 2^64, 2^64 + 1
```

All four should compare false against `2`, but InstSimplify folds lanes 2 and
3 as if they wrapped to `0` and `1`:

```llvm
ret <4 x i1> <i1 false, i1 false, i1 true, i1 true>
```

The same root also asserts for valid `i128` operands because the fold calls
`ConstantInt::getZExtValue()` even when the constant does not fit in
`uint64_t`.

This is an ordinary backend-independent InstSimplify/ConstantFolding
miscompile. It has no pointers, fast-math flags, or strictfp interaction.

Verifier: Franklin the 2nd (019e997f-e474-7730-8afe-79ec8d38b683) returned
YES.
