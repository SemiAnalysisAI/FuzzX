# LVI Load Hardening: `AnalyzeDefUseChain` checks `Def.Addr->getAttrs() & NodeAttrs::Dead` on parent, not `ChildDef`

## File
`llvm/lib/Target/X86/X86LoadValueInjectionLoadHardening.cpp`, lines 414-429.

## Code

```cpp
NodeAddr<InstrNode *> Owner{Use.Addr->getOwner(DFG)};
for (const auto &ChildDef :
     Owner.Addr->members_if(DataFlowGraph::IsDef, DFG)) {
  if (!DefsVisited.insert(ChildDef.Id).second)
    continue; // Already visited this def
  if (Def.Addr->getAttrs() & NodeAttrs::Dead)   // <-- BUG: checks `Def`, not `ChildDef`
    continue;
  if (Def.Id == ChildDef.Id)                     // <-- almost certainly meant SourceDef.Id
    continue; // `Def` uses itself (e.g., increment loop counter)

  AnalyzeDefUseChain(ChildDef);

  // `Def` inherits all of its child defs' transmitters.
  for (auto TransmitterId : Transmitters[ChildDef.Id])
    Transmitters[Def.Id].push_back(TransmitterId);
}
```

## Bug

Two suspicious checks inside the `ChildDef` loop:

1. **Dead-def filter is on the wrong def.** The intent of an "is dead" filter in this position is plainly to skip dead `ChildDef`s, but the code reads `Def.Addr->getAttrs() & NodeAttrs::Dead`. Because `Def` is loop-invariant inside this `for (ChildDef ...)` body, this is equivalent to `if (Def is dead) break;` — a hoist-style condition expressed as a per-iteration `continue`. The dead-def filter for `ChildDef` itself never runs, so the recursion happily descends into dead defs and inflates the `Transmitters` set with values that are never actually used by any non-dead consumer.

2. **Self-reference check compares the wrong defs.** The comment says "`Def` uses itself (e.g., increment loop counter)", which is the classic recursion-stopping condition. But `Def` and `ChildDef` cannot be the same NodeId here — `ChildDef` is a def belonging to `Owner` (the instruction that *uses* `Def`'s value), so its NodeId is structurally different from `Def.Id`. The only id that can usefully match `ChildDef.Id` here is the outermost `SourceDef.Id` (i.e., did the def-use chain cycle back to the analysis root?). As written, the check is a no-op, and recursion is bounded only by `DefsVisited.insert(ChildDef.Id).second`.

## Why it matters

Combined, the two errors mean the analysis can:
- Recurse into dead child defs (analysis cost / over-approximation), and
- Fail to short-circuit when reaching the analysis root through a loop carried def-use chain (potentially over-counting `Transmitters` for the same SourceDef).

Over-approximation produces extra (unneeded) `LFENCE` insertions, which is a perf bug, not a miscompile, but it is also an analysis correctness bug — the algorithm's stated invariants don't actually hold.

## Confidence

Medium-high (read-only analysis pass; the structural mismatch is unambiguous, but the effect under typical inputs is "extra fences" rather than a crash or miscompile).
