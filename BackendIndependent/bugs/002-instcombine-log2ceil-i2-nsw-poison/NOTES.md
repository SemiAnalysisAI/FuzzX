# 002 - log2-ceil idiom fold emits poison-generating `sub nsw` for `i2`

Component: `llvm/lib/Transforms/InstCombine/InstCombineAddSub.cpp:1966`

The log2-ceil idiom:

```llvm
zext(ctpop(A) != 1) + (ctlz(A, true) ^ (BW - 1))
```

is folded to a `BW - ctlz(A - 1, false)` shape that currently emits:

```llvm
%1 = add i2 %a, -1
%2 = call range(i2 0, -1) i2 @llvm.ctlz.i2(i2 %1, i1 false)
%add = sub nuw nsw i2 -2, %2
```

For `%a = 2`, the source is defined and returns `1`: `ctpop(2) = 1`,
`ctlz(2, true) = 0`, and `0 xor 1 = 1`. The target computes
`ctlz(1, false) = 1`, then `sub nsw i2 -2, 1`. Signed `i2` range is
`[-2, 1]`, so the mathematical result `-3` overflows and becomes poison.

Verifier: Kant (019e98e1-d54a-7c32-a076-6b485c79d98a) returned YES.
