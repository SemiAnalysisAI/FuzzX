# vector_reduce_mul fold of sext(<n x i1>) wrong when n is odd

**File:** `llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp`
**Lines:** 4112-4137 (the `Intrinsic::vector_reduce_mul` case)

## Reasoning

The fold rewrites
```
vector_reduce_mul(?ext(<n x i1> V))  -->  zext(vector_reduce_and(V))
```
unconditionally using `Builder.CreateZExt` regardless of whether the original
extension was `zext` or `sext`. For `zext`, lanes are 0/1 and the product is the
AND of the source bits, which the fold computes correctly. For `sext`, lanes
are 0/-1: if any lane is 0 the product is 0 (correct), but if *all* lanes are 1
the product is `(-1)^n`. When `n` is odd, the true product is `-1` (all-ones in
iM), yet the fold replaces it with `zext(true)` = 1. The comment immediately
above (lines 4117-4119) even says the fold should produce
"zext(vector_reduce_and(<n x i1>))", which is silently wrong for `sext` with
odd vector lengths. The `?ext` shorthand at line 4117 papers over the missing
sign-extension/negation branch.

## IR repro for `opt -passes=instcombine`

```llvm
define i8 @f(<3 x i1> %v) {
  %ext = sext <3 x i1> %v to <3 x i8>
  %r = call i8 @llvm.vector.reduce.mul.v3i8(<3 x i8> %ext)
  ret i8 %r
}
declare i8 @llvm.vector.reduce.mul.v3i8(<3 x i8>)
```

## Expected wrong outcome

For `%v = <i1 true, i1 true, i1 true>`:
- True semantics: `sext` yields `<i8 -1, i8 -1, i8 -1>`; the multiplicative
  reduction is `(-1) * (-1) * (-1) = -1` (i.e. `i8 255`).
- After `instcombine` the body becomes:
  ```
  %1 = bitcast <3 x i1> %v to i3
  %2 = icmp eq i3 %1, -1
  %r = zext i1 %2 to i8        ; returns 1 for all-true input
  ```
  which yields `1`, not `-1`. Divergence confirmed against `instsimplify`
  on the all-constant version (which still correctly returns `i8 -1`).

The correct fix is to choose between `zext`/`sext`/`mul-by-parity` based on the
original cast and the parity of the lane count: for `sext` input, the result
should be the `sext`/replicated-and-negated-on-odd-n value, e.g.
`sext(and-reduce(V)) * (-1)^n`, or simply not perform this transform when the
original cast is `sext` and the lane count is odd.
