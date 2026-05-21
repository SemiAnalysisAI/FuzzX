# foldShuffleOfShuffles returns implicit-bool from PoisonValue pointer

**File:** llvm/lib/Transforms/Vectorize/VectorCombine.cpp:3022-3023

```cpp
  if (!NewX)
    return PoisonValue::get(ShuffleDstTy);   // <-- bug
  if (!NewY)
    NewY = PoisonValue::get(ShuffleSrcTy);
```

## Reasoning

`foldShuffleOfShuffles` returns `bool`. When the merged mask ends up referencing
no real source (all outer-mask lanes resolve through inner masks to
PoisonMaskElem), `NewX` is still null. The author clearly intended to *replace
I with PoisonValue and report success*, but the statement
`return PoisonValue::get(ShuffleDstTy);` returns a non-null `PoisonValue*` that
the compiler implicitly converts to `true`. Crucially, `replaceValue(I, ...)`
is never called, so the original `shufflevector` is left in the IR while the
pass tells the pass manager that "we changed something" (causing analyses to
be invalidated and the user-supplied IR not optimized as intended). It is also
the missed-optimization the author plainly wrote code to perform: the result
must be poison and should have been replaced with `poison`. Compare with the
analogous `NewY` line right below, which correctly *assigns* rather than
returns.

Introduced by 10756d32f (David Green, 2026-05-16) per `git blame`.

## Repro

`opt -passes='vector-combine' -S` on:

```llvm
define <4 x i32> @test(<4 x i32> %x, <4 x i32> %y) {
  %a = shufflevector <4 x i32> %x, <4 x i32> poison,
                     <4 x i32> <i32 poison, i32 poison, i32 poison, i32 poison>
  %b = shufflevector <4 x i32> %y, <4 x i32> poison,
                     <4 x i32> <i32 poison, i32 poison, i32 poison, i32 poison>
  %r = shufflevector <4 x i32> %a, <4 x i32> %b, <4 x i32> <i32 0, i32 4, i32 1, i32 5>
  ret <4 x i32> %r
}
```

## Expected wrong outcome

The fold path triggers (debug log emits the candidate, function returns true so
the pass-manager invalidates analyses), yet the output IR still contains the
original `%a`/`%b`/`%r` triple unchanged --- the intended replacement of `%r`
with `poison` never happens. Confirmed locally with the in-tree `opt`: pass
runs without crashing but leaves the IR unmodified despite the function having
"succeeded". A correctly-written branch (`replaceValue(I,
*PoisonValue::get(ShuffleDstTy)); return true;`) would have folded the outer
shuffle to a `poison` constant for the return value.

Severity: at minimum compile-time/pipeline-state bug (analyses needlessly
invalidated, missed canonicalization to poison); risk of cascading miscompiles
if other folds in the same iteration order rely on the operand being marked
poison. A two-line fix.
