# w265: Reassociate copies root FMF onto rewritten intermediate FP nodes (no intersection)

## Summary

`ReassociatePass::RewriteExprTree` takes the root expression's full
`FastMathFlags` and re-stamps every changed intermediate node with them,
ignoring the per-operand FMF that was on the original ops. The minimum
requirement to enter reassociation is only `reassoc nsz`
(`hasFPAssociativeFlags`, Reassociate.cpp:156-159). Other flags such as
`nnan`, `ninf`, `arcp`, `contract` only need to be intersected — but the code
unconditionally copies the root's set instead.

LangRef:
> Whether or not a flag like nnan is present on any or all of the rewritten
> instructions is based on whether or not it is possible for said instruction
> to have a NaN input or output, given the original flags.

Stamping `nnan` on a rewritten intermediate whose operands could originally
have been NaN strengthens the contract — that intermediate now produces poison
instead of NaN, which is a miscompile-class change.

## Source

- `llvm/lib/Transforms/Scalar/Reassociate.cpp:729-740` — the post-rewrite
  flag-update loop. For FP, `Flags = I->getFastMathFlags()` and
  `ExpressionChangedStart->setFastMathFlags(Flags)` — the *root*'s flags are
  applied verbatim to each rewritten node; no intersection with the original
  operand flags happens.
- Compare `OverflowTracking::applyFlags`,
  `llvm/lib/Transforms/Utils/Local.cpp:4063-4074`: the integer path correctly
  uses `mergeFlags` (`HasNUW &= ...`) collected at
  `Reassociate.cpp:432-434` during `LinearizeExprTree`, so integer NUW/NSW are
  intersected.

The FP path has no analogous merge step: there is no
`FlagsAcc.intersectWith(BO->getFastMathFlags())` accumulation in
`LinearizeExprTree`.

## Reproducer

`/home/orenamd@semianalysis.com/FuzzX/x86/candidates/w265-reassoc-fp-root-flags-no-intersect.ll`

```llvm
define double @test(double %a, double %b, double %c, double %d) {
  %t1 = fadd reassoc nsz double %a, %b
  %t2 = fadd reassoc nsz double %c, %d
  %r  = fadd reassoc nsz nnan ninf double %t2, %t1
  ret double %r
}
```

`opt -passes=reassociate -S`:
```llvm
define double @test(double %a, double %b, double %c, double %d) {
  %t2 = fadd reassoc nnan ninf nsz double %b, %a
  %t1 = fadd reassoc nnan ninf nsz double %t2, %c
  %r  = fadd reassoc nnan ninf nsz double %t1, %d
  ret double %r
}
```

The original `%t1 = fadd reassoc nsz` did *not* have `nnan`/`ninf`; the
reassociated `%t2 = fadd reassoc nnan ninf nsz double %b, %a` (computing
`%b + %a`) now carries `nnan ninf`. If `%b + %a` evaluates to NaN
(e.g. `+inf + -inf`) or Inf at runtime, the rewritten instruction yields
poison whereas the original would have yielded the corresponding NaN/Inf.

Persists through `-O2`:
```
%t2 = fadd reassoc nnan ninf nsz double %b, %a
%t1 = fadd reassoc nnan ninf nsz double %t2, %c
%r  = fadd reassoc nnan ninf nsz double %t1, %d
```

## Fix sketch

In `LinearizeExprTree`, alongside `Flags.mergeFlags(*I)`, accumulate the
intersection of `cast<FPMathOperator>(I)->getFastMathFlags()` over every
visited reassociable op (and the leaves' flags if they are FPMathOperators).
Pass that intersected FMF set into `RewriteExprTree` and use it instead of
`I->getFastMathFlags()` at Reassociate.cpp:734-736 and at
Reassociate.cpp:706-707 (the new-`BinaryOperator` path).
