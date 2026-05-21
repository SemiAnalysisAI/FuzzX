# w631: MachineScheduler load/store clustering does not filter volatile/atomic MMOs in collectMemOpRecords

## Status
LATENT / lightly-guarded. `BaseMemOpClusterMutation::collectMemOpRecords`
admits volatile and atomic memops into the candidate pool. The downstream
reachability check via `Topo.IsReachable` and `addEdge`'s cycle check are the
only things that prevent clustering across the barrier-chain edges that
`buildSchedGraph` installs for ordered MMOs. No miscompile triggered in this
session — see "Triggers attempted" — but the absence of an MMO-level filter
in the collection step itself is a fragile contract: a target that returns
true from `getMemOperandsWithOffsetWidth` for an ordered op, plus a
`shouldClusterMemOps` that approves the pair, plus a missed Topo edge,
collectively becomes a miscompile.

## Code

`llvm/lib/CodeGen/MachineScheduler.cpp:2188`
```
void BaseMemOpClusterMutation::collectMemOpRecords(
    std::vector<SUnit> &SUnits, SmallVectorImpl<MemOpInfo> &MemOpRecords) {
  for (auto &SU : SUnits) {
    if ((IsLoad && !SU.getInstr()->mayLoad()) ||
        (!IsLoad && !SU.getInstr()->mayStore()))
      continue;
    // NB: no check for any MMO->isVolatile() / isAtomic() / !isUnordered()
    const MachineInstr &MI = *SU.getInstr();
    ...
    if (TII->getMemOperandsWithOffsetWidth(MI, BaseOps, Offset, ...))
      MemOpRecords.push_back(MemOpInfo(&SU, BaseOps, Offset, ...));
  }
}
```

`llvm/lib/CodeGen/MachineScheduler.cpp:2078`
```
void BaseMemOpClusterMutation::clusterNeighboringMemOps(...) {
  for (...) {
    auto MemOpa = MemOpRecords[Idx];
    unsigned NextIdx = Idx + 1;
    for (; NextIdx < End; ++NextIdx)
      if (!SUnit2ClusterInfo.count(MemOpRecords[NextIdx].SU->NodeNum) &&
          (FastCluster ||
           (!DAG->IsReachable(MemOpRecords[NextIdx].SU, MemOpa.SU) &&
            !DAG->IsReachable(MemOpa.SU, MemOpRecords[NextIdx].SU))))
        break;
    ...
    if (!TII->shouldClusterMemOps(MemOpa.BaseOps, ...))
      continue;
    ...
    if (!DAG->addEdge(SUb, SDep(SUa, SDep::Cluster)))
      continue;
```

The protection chain that actually keeps volatile/atomic apart:

1. `buildSchedGraph` flags volatile/ordered MMOs through `isGlobalMemoryObject`
   and chains them through `BarrierChain` (line 920-938 of
   ScheduleDAGInstrs.cpp).
2. Those barrier predecessor edges enter `Topo`.
3. `IsReachable` therefore returns true for any candidate pair separated by a
   barrier, so `clusterNeighboringMemOps` skips them.
4. As a final fallback, `addEdge` itself bails when adding the Cluster edge
   would form a cycle.

The fragility: if `FastCluster` is in effect (line 2222-2225 — triggered when
`MemOps.size() * DAG->SUnits.size() / 1000 > FastClusterThreshold` ≈ many
memops in a huge region), the `IsReachable` check at line 2098-2099 is
SKIPPED in favor of a same-chain-pred bucketing scheme at
`groupMemOps` line 2229-2240 that only inspects ctrl predecessors:

```
for (const SDep &Pred : MemOp.SU->Preds) {
  if ((Pred.isCtrl() && (IsLoad ||
        (Pred.getSUnit() && Pred.getSUnit()->getInstr()->mayStore()))) &&
      !Pred.isArtificial()) {
    ChainPredID = Pred.getSUnit()->NodeNum;
    break;
  }
}
```

This buckets by the FIRST non-artificial ctrl predecessor only. Two
volatile loads each with their own distinct `BarrierChain` predecessor SU
(because each became a new BarrierChain on the way through buildSchedGraph)
would fall into different buckets and not cluster. But if both share the
same chain-pred (e.g. both depend on the same earlier barrier), they may
end up in the same group, and the topological check is gone. The remaining
guard is `addEdge`'s cycle check, which only prevents *cycles*, not
semantic violations of the barrier ordering itself: if SUa and SUb are
both successors of the same barrier with no edge between them, adding a
Cluster edge does not create a cycle, but it would force them adjacent.
Whether that adjacency violates anything depends on what got chained
between them — which has been discarded in FastCluster mode.

## Triggers attempted (no miscompile observed)

Two volatile i32 loads to adjacent offsets, x86_64 znver5:

```
define i64 @vol_loads(ptr noalias %p) {
  %p2 = getelementptr inbounds i32, ptr %p, i64 1
  %a = load volatile i32, ptr %p, align 4
  %b = load volatile i32, ptr %p2, align 4
  %ax = zext i32 %a to i64
  %bx = zext i32 %b to i64
  %sh = shl i64 %bx, 32
  %r = or i64 %sh, %ax
  ret i64 %r
}
```

`llc -mcpu=znver5 -O2 -stop-after=machine-scheduler` gives:
```
MOV32rm %0, 1, $noreg, 0, $noreg :: (volatile load (s32) from %ir.p11)
MOV32rm %0, 1, $noreg, 4, $noreg :: (volatile load (s32) from %ir.p2)
```

Order preserved; on x86 the regular MOV is the correct volatile lowering, and
adjacent ordering is also fine for two volatile loads as long as the program
order between them is preserved (which it was). To actually exploit the
gap would require:

- A target whose volatile/atomic load goes through `getMemOperandsWithOffsetWidth`
  returning true (most do).
- A region large enough to trip `FastCluster`.
- Two volatile loads with the same single non-artificial ctrl predecessor.
- A non-trivial `shouldClusterMemOps` return.

## Source citations

- `llvm/lib/CodeGen/MachineScheduler.cpp:2188-2216` — `collectMemOpRecords` (no MMO filter).
- `llvm/lib/CodeGen/MachineScheduler.cpp:2078-2169` — `clusterNeighboringMemOps`.
- `llvm/lib/CodeGen/MachineScheduler.cpp:2219-2247` — `groupMemOps` (FastCluster bucketing).
- `llvm/lib/CodeGen/ScheduleDAGInstrs.cpp:920-938` — `isGlobalMemoryObject` → BarrierChain.
- `llvm/lib/CodeGen/ScheduleDAGInstrs.cpp:1239-1250` — `addEdge` cycle check.

## Suggested defensive fix

Filter in `collectMemOpRecords`:
```cpp
if (any_of(MI.memoperands(), [](const MachineMemOperand *MMO) {
      return !MMO->isUnordered();
    }))
  continue;
```
Cheap, and removes the FastCluster fragility entirely.
