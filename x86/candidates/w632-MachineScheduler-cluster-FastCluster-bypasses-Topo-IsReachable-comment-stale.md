# w632: MachineScheduler clustering FastCluster bypasses Topo::IsReachable; in-source comment claims invariant that no longer holds

## Status
COMMENT BUG / latent fragility. The reasoning that justifies `IsLoad`'s
"copy successors only" and `IsStore`'s "no memory dependency to worry about"
explicitly relies on the `IsReachable` filter — which `FastCluster` skips.

## Code

`llvm/lib/CodeGen/MachineScheduler.cpp:2222`
```
bool FastCluster =
    ForceFastCluster ||
    MemOps.size() * DAG->SUnits.size() / 1000 > FastClusterThreshold;
```

`llvm/lib/CodeGen/MachineScheduler.cpp:2096`
```
for (; NextIdx < End; ++NextIdx)
  if (!SUnit2ClusterInfo.count(MemOpRecords[NextIdx].SU->NodeNum) &&
      (FastCluster ||
       (!DAG->IsReachable(MemOpRecords[NextIdx].SU, MemOpa.SU) &&
        !DAG->IsReachable(MemOpa.SU, MemOpRecords[NextIdx].SU))))
    break;
```

When `FastCluster` is true, the `IsReachable` clause short-circuits — any
candidate that has not already been clustered passes the gate. The fallback
filter at that point is purely the `groupMemOps` chain-pred bucketing
(line 2222-2247), which inspects only the SU's first non-artificial ctrl
predecessor.

`llvm/lib/CodeGen/MachineScheduler.cpp:2148`
```
} else {
  // Copy predecessor edges from SUb to SUa to avoid the SUnits that
  // SUb dependent on scheduled in-between SUb and SUa. Successor edges
  // do not need to be copied from SUa to SUb since no one will depend
  // on stores.
  // Notice that, we don't need to care about the memory dependency as
  // we won't try to cluster them if they have any memory dependency.
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                       This is only true with FastCluster=false.
```

The comment is fine in slow mode (the `IsReachable` clauses exclude pairs
that have any transitive chain edge between them). In FastCluster mode it is
not enforced: two SUs that share an unrelated chain-pred can be reachable
through chain edges yet still be clustered, and the predecessor-copying
loop then drags every predecessor of the second store onto the first store
as Artificial edges — including chain preds the first store didn't have.
The schedule becomes more constrained than the DAG built by
`buildSchedGraph` had asked for. In adversarial cases this might mask a
data dependency that was supposed to be observed between SUa and the
predecessor that just got artificially attached to it, by forcing SUa
later than SUa's own true predecessors permit. It is unlikely to be a
correctness bug because Artificial edges still add a topological
constraint rather than remove one — but the comment incorrectly suggests
this code is unreachable for memory-dependent stores.

## Recommendation

Either:
- Remove FastCluster's bypass of the IsReachable check (it was only a
  compile-time heuristic), or
- Repeat the IsReachable check inside `clusterNeighboringMemOps` for the
  pair-to-pair decision in FastCluster mode (cheap once you already have
  a small candidate list per bucket), or
- Update the comment to reflect the actual invariant.

## Source citations

- `llvm/lib/CodeGen/MachineScheduler.cpp:2078-2169` — `clusterNeighboringMemOps`.
- `llvm/lib/CodeGen/MachineScheduler.cpp:2219-2247` — `groupMemOps`.
- `llvm/lib/CodeGen/MachineScheduler.cpp:2148-2161` — stale comment.

## Triggers

Hard to synthesize via small-IR fuzzing because FastCluster needs many
memops in a single region. Synthetic test would need:
- A region with hundreds of memops.
- Two adjacent stores reachable from each other via a common chain pred
  but with intervening unrelated stores.

Not produced in 25-minute time-box.
