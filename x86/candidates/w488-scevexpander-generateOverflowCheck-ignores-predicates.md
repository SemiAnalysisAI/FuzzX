# SCEVExpander::generateOverflowCheck silently discards the SCEV predicates used to compute its trip count

File: `llvm/lib/Transforms/Utils/ScalarEvolutionExpander.cpp`
Function: `SCEVExpander::generateOverflowCheck`, lines 2227-2336.

## The transform

`generateOverflowCheck(AR, Loc, Signed)` synthesizes a runtime check that an
affine AddRec `{Start,+,Step}<L>` will not wrap over its trip count. It
fetches the trip count from:

```cpp
// Lines 2232-2237 (verbatim):
//
//   // FIXME: It is highly suspicious that we're ignoring the predicates here.
//   SmallVector<const SCEVPredicate *, 4> Pred;
//   const SCEV *ExitCount =
//       SE.getPredicatedSymbolicMaxBackedgeTakenCount(AR->getLoop(), Pred);
//
//   assert(!isa<SCEVCouldNotCompute>(ExitCount) && "Invalid loop count");
```

The `Pred` vector is **populated by SCEV** with the predicates that must
hold for `ExitCount` to be valid (e.g., wrap-predicates on a different IV
participating in the loop's exit calc), and then **never used** by
`generateOverflowCheck`. The function continues to build a multiply-with-
overflow on `|Step| * ExitCount` and compare against `Start`, as if the
unpredicated symbolic max BTC were unconditionally correct.

Callers of `generateOverflowCheck` are:
1. `expandWrapPredicate(Pred, IP)` (line 2338). The wrap predicate
   represents `IncrementNUSW`/`IncrementNSSW` for AR. The caller emits
   either or both of the overflow checks based on `Pred->getFlags()`.
2. `expandUnionPredicate(Union, IP)` (line 2363), which OR-combines
   `expandCodeForPredicate` results for each predicate in `Union`.

Neither caller has any awareness that `generateOverflowCheck` internally
relied on an *additional* SCEV predicate set (the discarded `Pred` vector).
Those additional predicates are silently dropped — they are not added to
the union of checks emitted to the IR.

## Why this miscompiles

LoopVectorize and LoopVersioning rely on `expandUnionPredicate` to emit a
guard that, if false, lets the optimized path execute. The guarantee is
"all SCEV facts the optimized path depended on are runtime-checked." When
`generateOverflowCheck` consumes a *predicated* BTC, those extra
predicates become implicit dependencies of the optimized path that are
**not** runtime-checked.

If the implicit predicate is, e.g., "RHS IV has NUSW", and the actual
runtime input violates it, the optimized path's wrap check evaluates with
a `TripCountVal` that is *not* a valid trip count of the loop — leading
to a wrap check that gives the wrong answer. The vectorized loop then
executes despite running with an out-of-range IV; addresses computed by
the inner GEP can step outside the buffer.

## Source-self-confirming evidence

The FIXME at line 2232 is verbatim: `// FIXME: It is highly suspicious
that we're ignoring the predicates here.` This is a known acknowledgement
that the call is unsound, kept as a TODO without fixing for years.

`expandUnionPredicate` (line 2363) does not consult `generateOverflowCheck`'s
internal predicate vector — its full implementation is:

```cpp
Value *SCEVExpander::expandUnionPredicate(const SCEVUnionPredicate *Union,
                                          Instruction *IP) {
  SmallVector<Value *> Checks;
  for (const auto *Pred : Union->getPredicates()) {
    Checks.push_back(expandCodeForPredicate(Pred, IP));
    Builder.SetInsertPoint(IP);
  }
  if (Checks.empty())
    return ConstantInt::getFalse(IP->getContext());
  return Builder.CreateOr(Checks);
}
```

Nothing here observes or propagates the predicates that
`generateOverflowCheck` discarded.

## Why this is in-scope for x86

The miscompile manifests as wrong-answer behavior in vectorized loops at
`-O2`/`-O3` on x86: the runtime overflow-check guard chooses the vector
path when the scalar path would have detected an overflow on the
multiplied trip count, because the trip count itself was only valid under
an unchecked SCEV predicate.

## How to hunt

Fuzz IR with:
- A loop containing two IVs of mismatched types/widths.
- An exit condition expressed as `icmp` of a SCEV expression that requires
  a Wrap predicate on the secondary IV to make
  `getPredicatedSymbolicMaxBackedgeTakenCount` succeed.
- A primary AddRec whose SCEVWrapPredicate is registered with
  `PredicatedScalarEvolution`, then expanded via `expandUnionPredicate`.

Compare the emitted runtime check against the BTC predicate vector that
`generateOverflowCheck` discarded; the difference is the unsound subset.

## Status: source-confirmed via FIXME in the source itself.

Mechanically certain. Fuzz repro requires invoking LoopVersioning/
LoopVectorize on a predicated AddRec setup, which is a more elaborate
construction than a single-pass `opt -passes=indvars` invocation.
