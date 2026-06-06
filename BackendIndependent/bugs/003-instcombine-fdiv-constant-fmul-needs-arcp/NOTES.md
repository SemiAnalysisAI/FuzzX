# 003 - FDiv-by-constant reassociation lacks `arcp`

Component: `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:814`

InstCombine folds:

```llvm
(X / C1) * C  -->  X * (C / C1)
```

when both FP operations have `reassoc`. The source even has a FIXME saying this
seems like it should check `arcp`.

LangRef gives `arcp` the specific permission to treat division as multiplication
by a reciprocal. `reassoc` alone permits algebraic reassociation, but not this
reciprocal rounding change. For `x = 7.0f`, `(x / 10.0f) * 3.0f` and
`x * 0.3f` differ by one ulp.

Verifier: Gauss (019e98e1-d5e3-7b50-b5b2-190c295baa73) returned YES.
