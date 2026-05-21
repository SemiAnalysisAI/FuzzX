# w266: Reassociate creates a new FP `factor` mul stamped with root FMF (no intersection)

## Summary

When Reassociate factors a repeated FP addend (`a + a + ...`) into a multiply,
it creates a brand-new `fmul` instruction via `CreateMul(TheOp, C, "factor",
..., I)`. `CreateMul` calls `setFastMathFlags(cast<FPMathOperator>(FlagsOp)->
getFastMathFlags())` where `FlagsOp = I` is the *root* fadd. The factor
multiply did not exist in the original program, so any FMF on it must be the
intersection of flags from the original instructions whose effect it now
encodes — not the root's full set.

Result: a synthetic `fmul nnan ninf arcp` can appear even though no original
op carried `nnan`/`ninf`/`arcp`. NaN/Inf operands that originally produced
finite-or-NaN output now produce poison.

## Source

- `llvm/lib/Transforms/Scalar/Reassociate.cpp:263-274` — `CreateMul`:
  ```cpp
  Res->setFastMathFlags(cast<FPMathOperator>(FlagsOp)->getFastMathFlags());
  ```
  Called for FP factoring at:
- `llvm/lib/Transforms/Scalar/Reassociate.cpp:1518` —
  `Instruction *Mul = CreateMul(TheOp, C, "factor", I->getIterator(), I);`
  (`OptimizeAdd`, the `MaxOcc > 1` factoring branch).
- `llvm/lib/Transforms/Scalar/Reassociate.cpp:1695` —
  `Instruction *V2 = CreateMul(V, MaxOccVal, "reass.mul", I->getIterator(), I);`
  (cross-add factor extraction).

Equivalent for `CreateAdd` (called from `BreakUpSubtract` at line 1006 and
`EmitAddTreeOfValues` at line 1084) — `setFastMathFlags(FlagsOp)` at
Reassociate.cpp:258.

## Reproducer

`/home/orenamd@semianalysis.com/FuzzX/x86/candidates/w266-reassoc-fp-factor-new-flags-from-root.ll`

```llvm
define double @test(double %a, double %b, double %c, double %d, double %e) {
  %t1 = fadd reassoc nsz double %a, %b
  %t2 = fadd reassoc nsz double %t1, %a
  %t3 = fadd reassoc nsz double %t2, %c
  %t4 = fadd reassoc nsz double %t3, %d
  %r  = fadd reassoc nsz nnan ninf arcp double %t4, %e
  ret double %r
}
```

`opt -passes=reassociate -S`:
```llvm
define double @test(double %a, double %b, double %c, double %d, double %e) {
  %factor = fmul reassoc nnan ninf nsz arcp double %a, 2.000000e+00
  %t2 = fadd reassoc nnan ninf nsz arcp double %factor, %b
  %t3 = fadd reassoc nnan ninf nsz arcp double %t2, %c
  %t4 = fadd reassoc nnan ninf nsz arcp double %t3, %d
  %r  = fadd reassoc nnan ninf nsz arcp double %t4, %e
  ret double %r
}
```

- `%factor = fmul ... nnan ninf arcp` is a *new* instruction. None of the
  original ops carried `nnan`, `ninf`, or `arcp`.
- The original `%a + %a` (intermediate of the chain) allowed NaN: e.g. when
  `%a` is NaN, the result was NaN. `2.0 * NaN` with `nnan` is poison —
  semantics change.
- `arcp` permits reciprocal substitution: pulling `arcp` onto a `fmul` that
  did not exist allows downstream passes to assume the multiply is exact under
  arcp transforms.

Persists through `-O2` (the same IR survives the rest of the pipeline).

## Fix sketch

`CreateAdd`/`CreateMul`/`CreateNeg` should accept a precomputed `FastMathFlags`
intersection rather than a single `FlagsOp` instruction. All call sites that
synthesize a *new* op (factor mul, reass.mul, BreakUpSubtract neg, add tree)
should pass the intersected FMF of the original participating operands, not
the root.
