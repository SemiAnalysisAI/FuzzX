# w602 — MachineBlockFrequencyInfo::onEdgeSplit overwrites successor frequency

## Target
- `llvm/lib/CodeGen/MachineBlockFrequencyInfo.cpp:289-298`
- callers:
  - `llvm/lib/CodeGen/PHIElimination.cpp:867-871`
  - `llvm/lib/CodeGen/MachineSink.cpp:865-871`

## Mechanism

```
289 void MachineBlockFrequencyInfo::onEdgeSplit(
290     const MachineBasicBlock &NewPredecessor,
291     const MachineBasicBlock &NewSuccessor,
292     const MachineBranchProbabilityInfo &MBPI) {
293   assert(MBFI && "Expected analysis to be available");
294   auto NewSuccFreq = MBFI->getBlockFreq(&NewPredecessor) *
295                      MBPI.getEdgeProbability(&NewPredecessor, &NewSuccessor);
296
297   MBFI->setBlockFreq(&NewSuccessor, NewSuccFreq);
298 }
```

The API contract (per its only two callers) is: `NewSuccessor` is the
brand-new block inserted in the middle of the critical edge
`NewPredecessor -> oldDest` by `MachineBasicBlock::SplitCriticalEdge`.
Because it is brand-new it has exactly one predecessor (NewPredecessor),
and the edge probability MBPI returns for `NewPredecessor -> NewSuccessor`
inherits from the original `NewPredecessor -> oldDest` probability.

In that *intended* setting the computation
`NewSuccFreq = pred_freq * edge_prob` is the correct frequency for a
single-predecessor block.

### The latent bug

Nothing in `onEdgeSplit` *checks* that NewSuccessor really has a single
predecessor or that it was freshly created.  It is exposed as a public
API on the analysis (`MachineBlockFrequencyInfo.h:87`).  A future
caller — or a misuse where the same NewSuccessor is reached via more
than one split-attempt in the same pass — would silently lose the
contribution of the other predecessors:

- `setBlockFreq` is an unconditional assignment, not an accumulation
- `MBPI.getEdgeProbability(pred, succ)` does not (and cannot) account for
  the relative weighting of multiple predecessors

If `NewSuccessor` truly has predecessors {NewPredecessor, P2, P3, ...}
the correct frequency is `sum_i(freq(P_i) * P(P_i -> NewSuccessor))`,
not just the first term.

### Why no observable bug at default -O2

`PHIElimination.cpp:870` calls it immediately after a successful
`SplitCriticalEdge`; in that path the new block is single-predecessor by
construction.  `MachineSink.cpp:870` likewise.  So the contract is
satisfied today.

## Suggested hardening

Either:

1. Document the precondition in the header (`NewSuccessor` must be a
   freshly-split critical-edge block with a unique predecessor
   `NewPredecessor`), and `assert(NewSuccessor.pred_size() == 1 &&
   *NewSuccessor.pred_begin() == &NewPredecessor)`; or

2. Rewrite the body to sum over *all* predecessors:

```cpp
BlockFrequency F(0);
for (const MachineBasicBlock *P : NewSuccessor.predecessors())
  F += MBFI->getBlockFreq(P) * MBPI.getEdgeProbability(P, &NewSuccessor);
MBFI->setBlockFreq(&NewSuccessor, F);
```

The second form is also robust to chained edge splits in a single pass
and removes the implicit single-pred precondition.

## Files
None — issue is in source-level invariant, not directly reproducible
via .ll diff under default -O2 because the only in-tree callers honor
the precondition.
