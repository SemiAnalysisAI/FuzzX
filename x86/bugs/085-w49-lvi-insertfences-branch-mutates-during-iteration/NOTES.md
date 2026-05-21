# LVI Load Hardening: `insertFences` mutates `CutEdges` while iterating over the same edge container

## File
`llvm/lib/Target/X86/X86LoadValueInjectionLoadHardening.cpp`, lines 718-760.

## Code

```cpp
int X86LoadValueInjectionLoadHardeningImpl::insertFences(
    MachineFunction &MF, MachineGadgetGraph &G,
    EdgeSet &CutEdges /* in, out */) const {
  int FencesInserted = 0;
  for (const Node &N : G.nodes()) {
    for (const Edge &E : N.edges()) {
      if (CutEdges.contains(E)) {
        ...
        } else if (MI->isBranch()) { // insert the LFENCE before the branch
          MBB = MI->getParent();
          InsertionPt = MI;
          Prev = MI->getPrevNode();
          // Remove all egress CFG edges from this branch because the inserted
          // LFENCE prevents gadgets from crossing the branch.
          for (const Edge &E : N.edges()) {
            if (MachineGadgetGraph::isCFGEdge(E))
              CutEdges.insert(E);          // <-- mutates CutEdges
          }
        }
        ...
      }
    }
  }
  return FencesInserted;
}
```

## Bug

The outer loop iterates `N.edges()` and tests `CutEdges.contains(E)` on each visited edge. When the current edge `E` is "the branch egress that triggered the fence," the inner block inserts *every* CFG edge of `N` into `CutEdges`, including edges later in `N.edges()` that have not yet been visited by the outer iteration.

Those newly-inserted edges are then re-visited in subsequent iterations of the outer loop with `CutEdges.contains(E)` now true. Two effects:

1. **Duplicate LFENCE bookkeeping** — for each not-yet-visited CFG edge of `N` that the "branch" arm just inserted, the outer iteration re-enters the `if (CutEdges.contains(E))` body. The "no redundant fence" guard at line 751 (`!isFence(&*InsertionPt) && (!Prev || !isFence(Prev))`) catches the case where an LFENCE has just been emitted in this MBB and reads it back, so the second visit does not re-emit. Good.
2. **But**: the second visit happens to take the `else { // insert the LFENCE after the instruction }` branch (since the branch was already processed once doesn't matter — `MI` is the branch, `MI->isBranch()` is still true). So the second visit re-runs the branch arm and adds *all CFG edges* again to `CutEdges` (no-ops since already in `CutEdges`), then re-checks the redundancy guard.

So the immediate effect is wasted work, not a miscompile. **However**, the redundancy guard is fragile: if any *non-LFENCE* instruction were emitted between `MI` and the previously inserted LFENCE (e.g., by a later refactor that interleaves work), the guard would silently emit a duplicate LFENCE for every egress branch edge.

More importantly, the bookkeeping invariant of `CutEdges` (as documented `/* in, out */`) is mutated during iteration of the same `EdgeSet`-backed container used to drive the outer loop in `hardenLoadsWithPlugin`. After `insertFences` returns, `GraphBuilder::trim(*Graph, NodeSet{*Graph}, CutEdges)` consumes the now-augmented `CutEdges`. If the augmentation is wrong (e.g., the branch was a conditional whose "fall-through" CFG edge was *not* selected by the heuristic for cutting), we silently elide that edge from the post-trim graph and the next plugin iteration treats it as resolved.

## Why it matters

The plugin path (`hardenLoadsWithPlugin`) loops until `Graph->NumGadgets == 0`. Each iteration trims the graph using the mutated `CutEdges`. If `insertFences` over-adds egress edges that the heuristic did not request, the next iteration sees a smaller graph than it should and may terminate without cutting still-live gadgets — same security-correctness concern as the previous candidate.

## Confidence

Low-medium. The mutation-during-iteration is unambiguous; whether it produces a real miscut depends on whether `EdgeSet` (an LLVM `BitVector`-backed type) preserves stable iteration semantics when bits are set during iteration over the source `N.edges()` (which iterates the underlying `ImmutableGraph` edges, not `CutEdges` itself — so iteration is over the graph's edge array, not over `CutEdges`). That makes the iteration itself safe; the concern collapses to "does over-adding CFG edges to `CutEdges` after this fence is emitted alter the plugin's per-iteration accounting in a security-visible way?" Worth a focused look by the LVI author.
