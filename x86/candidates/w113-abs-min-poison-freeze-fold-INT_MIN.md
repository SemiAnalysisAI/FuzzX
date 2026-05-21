# Candidate: visitABS_MIN_POISON freeze-of-abs_min_poison fold drops INT_MIN poison guard

File: llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:12149-12170

## Source pattern (lines 12166-12167)

```cpp
  // fold (abs_min_poison (freeze (abs x))) -> (freeze (abs x))
  // fold (abs_min_poison (freeze (abs_min_poison x))) ->
  //   (freeze (abs_min_poison x))
  if (ISD::isAbsOpcode(peekThroughFreeze(N0).getOpcode()))
    return N0;
```

`isAbsOpcode` recognises ABS *and* ABS_MIN_POISON, and `peekThroughFreeze`
looks through ISD::FREEZE. The combiner therefore folds
`abs_min_poison(freeze(abs_min_poison X))` into `freeze(abs_min_poison X)`.

The accompanying comment claims this is sound because "freeze already
consumed the poison". That reasoning is incomplete:

- For X == INT_MIN, inner `abs_min_poison X` produces poison; FREEZE then
  picks SOME deterministic non-poison value V (the choice is permitted to be
  any value of the type, including INT_MIN itself).
- The original program guarantees the outer `abs_min_poison` then produces
  poison for V == INT_MIN, and `|V|` otherwise — i.e. the outer poisons
  exactly when V == INT_MIN.
- The folded program is just V — the outer guard is gone. If V == INT_MIN,
  the original would poison (giving the downstream code license to fault /
  optimise) but the folded program emits INT_MIN as a normal value.

This is a refinement in the wrong direction: poison was replaced with a
specific concrete value, which constrains downstream consumers more
strongly than the original IR allowed. Any downstream consumer that uses
`isGuaranteedNotToBeUndefOrPoison` on the result of the outer
`abs_min_poison` may now reason incorrectly.

## Single-freeze case

`abs_min_poison(freeze(abs X))`:
- inner `abs X` is well-defined (no poison even on INT_MIN; ABS wraps).
- FREEZE is a no-op for a guaranteed-non-poison value.
- outer `abs_min_poison(|X|)`: poisons iff |X| == INT_MIN, which can
  happen only when X == INT_MIN (then `abs(X) == INT_MIN`).
- Folded form: `freeze(abs X)` — no INT_MIN guard.

So even with `abs` (not `abs_min_poison`) on the inside the comment's claim
is wrong: the absolute value of INT_MIN in a wrap-on-overflow ABS is still
INT_MIN, and the outer `abs_min_poison` was supposed to detect that.

## Why this is hard to observe as an asm diff

`ISD::ABS_MIN_POISON` is rarely produced by upstream IR-to-DAG
translation today (most paths produce ISD::ABS). The fold sits as a latent
trap for any frontend / pass that does emit ABS_MIN_POISON wrapped in
FREEZE.

## Recommended fix

Restrict the second arm of the fold to inner ABS only (`peekThroughFreeze`
followed by `Opcode == ISD::ABS`), or strengthen the guarantee on the
frozen value (e.g. require a known non-INT_MIN). The comment justification
needs to be revisited too — freeze does NOT make the outer
`abs_min_poison` redundant for INT_MIN inputs.
