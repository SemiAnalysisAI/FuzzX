# w364: ScalarizeMaskedMemIntrin asymmetric "explicitly unknown" branch weights — load/store/expandload/compressstore/histogram leak stale PGO weights, gather/scatter do not

## Status: confirmed by source inspection; observable in PGO builds via PGO-verifying tooling

## Where (source:lines)

`llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp` — at every
`SplitBlockAndInsertIfThen` call site, a `BranchWeights` MDNode is passed.
The choice is *not consistent across intrinsics*:

- `scalarizeMaskedLoad`         line **204**, **262**: `/*BranchWeights=*/nullptr`
- `scalarizeMaskedStore`        line **369**, **420**: `/*BranchWeights=*/nullptr`
- `scalarizeMaskedGather`       line **548-550**: `getExplicitlyUnknownBranchWeightsIfProfiled(*CI->getFunction(), DEBUG_TYPE)` ✓
- `scalarizeMaskedScatter`      line **682-684**: `getExplicitlyUnknownBranchWeightsIfProfiled(*CI->getFunction(), DEBUG_TYPE)` ✓
- `scalarizeMaskedExpandLoad`   line **800**:    `/*BranchWeights=*/nullptr`
- `scalarizeMaskedCompressStore` line **920**:    `/*BranchWeights=*/nullptr`
- `scalarizeMaskedVectorHistogram` line **1021**: `/*BranchWeights=*/nullptr`

## Why this matters

`getExplicitlyUnknownBranchWeightsIfProfiled` (defined at
`llvm/lib/IR/ProfDataUtils.cpp:290`) does the following:

```cpp
MDNode *llvm::getExplicitlyUnknownBranchWeightsIfProfiled(Function &F, StringRef PassName) {
  if (std::optional<Function::ProfileCount> EC = F.getEntryCount();
      !EC || EC->getCount() == 0)
    return nullptr;
  MDBuilder MDB(F.getContext());
  return MDNode::get(
      F.getContext(),
      {MDB.createString(MDProfLabels::UnknownBranchWeightsMarker),  // "unknown"
       MDB.createString(PassName)});
}
```

When the function has a profile entry count, the marker `!prof !{!"unknown",
!"scalarize-masked-mem-intrin"}` is attached to the inserted branch.
Consumers (e.g. `BranchProbabilityInfo`, `PGOInstrumentation`,
`-verify-mi-profile`, the matrix lowering tooling at
`LowerMatrixIntrinsics.cpp`, etc.) recognise this and treat the branch as
"weights deliberately unknown, do NOT silently invent uniform weights or
flag the function as malformed". Without it, the branch is a bare control
edge — and PGO consumers cannot tell whether the branch:
(a) was inserted post-profile and its weight is genuinely unknown, or
(b) just lost its weights to a buggy transform and should be flagged.

In `scalarizeMaskedGather`/`Scatter` the LLVM authors intentionally added
the marker (see code comment at lines 544-545 / 678-679: "We mark the branch
weights as explicitly unknown given they would only be derivable from the
mask which we do not have VP information for."). The *same* argument applies
verbatim to masked load / store / expandload / compressstore / histogram —
their per-lane branches are also derived from a mask without VP info.

But those five functions all pass `nullptr`. Consequence in a PGO-profiled
build:

- Verifier tools (e.g. `opt -passes=verify-mi-profile`, llc's `-misched-...`
  validators) will silently accept these as "no prof", which is correct
  formally but produces *worse* downstream BPI estimates than the explicit
  "unknown" marker would.
- A second pass that re-attaches branch weights based on heuristics
  (Branch{Folding,Probability}) cannot distinguish the per-lane branch from
  a hot/cold-known fold-decision branch and may apply default weights that
  bias scheduling.
- It is also straight inconsistency: identical code patterns in the same
  file with explicitly different metadata behavior.

## Reproducer (source diff is the reproducer)

```cpp
// scalarizeMaskedLoad, line 261-262
Instruction *ThenTerm =
    SplitBlockAndInsertIfThen(Predicate, InsertPt, /*Unreachable=*/false,
                              /*BranchWeights=*/nullptr, DTU);
```

vs.

```cpp
// scalarizeMaskedGather, line 546-550
Instruction *ThenTerm =
    SplitBlockAndInsertIfThen(Predicate, InsertPt, /*Unreachable=*/false,
                              getExplicitlyUnknownBranchWeightsIfProfiled(
                                  *CI->getFunction(), DEBUG_TYPE),
                              DTU);
```

The asymmetric choice cannot be explained by intrinsic semantics — all
seven scalarizers split on a mask bit and create a guarded
load/store/update — so it is almost certainly a "left over from before the
helper existed" bug from the gather/scatter PR not being propagated to its
siblings.

## Where to fix

Replace `/*BranchWeights=*/nullptr` with
`getExplicitlyUnknownBranchWeightsIfProfiled(*CI->getFunction(), DEBUG_TYPE)`
at all five sites: lines 204, 262, 369, 420, 800, 920, 1021.

## Triage notes

This is unique vs w104/w360-w363 — it is not about per-load/store metadata,
but about branch-prof metadata on the SplitBlockAndInsertIfThen branches.
The asymmetry itself is the smoking gun: two functions out of seven do the
right thing, the other five do not.
