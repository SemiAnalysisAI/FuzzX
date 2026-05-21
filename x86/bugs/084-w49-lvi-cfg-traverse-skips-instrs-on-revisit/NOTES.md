# LVI Load Hardening: `TraverseCFG` skips intra-block gadget components when a block is re-entered via a back edge

## File
`llvm/lib/Target/X86/X86LoadValueInjectionLoadHardening.cpp`, lines 492-525.

## Code

```cpp
SmallPtrSet<MachineBasicBlock *, 8> BlocksVisited;
std::function<void(MachineBasicBlock *, GraphIter, unsigned)> TraverseCFG =
    [&](MachineBasicBlock *MBB, GraphIter GI, unsigned ParentDepth) {
      unsigned LoopDepth = MLI.getLoopDepth(MBB);
      if (!MBB->empty()) {
        // Always add the first instruction in each block
        auto NI = MBB->begin();
        auto BeginBB = MaybeAddNode(&*NI);
        Builder.addEdge(ParentDepth, GI, BeginBB.first);
        if (!BlocksVisited.insert(MBB).second)
          return;          // <-- early-return on revisit
        ...
        // Add any instructions within the block that are gadget components
        // Add terminator, etc.
      }
      for (MachineBasicBlock *Succ : MBB->successors())
        TraverseCFG(Succ, GI, LoopDepth);
    };
```

## Bug

When `MBB` is revisited (e.g., a loop back-edge or join), the function:

1. Always emits the CFG edge `ParentDepth: GI -> BeginBB`. Good — that edge must be present so the second-entry path is represented.
2. Then **returns immediately**, skipping the loop over `MBB`'s successors.

Consequence: paths that flow through `MBB` *via the second visit* are never propagated to its successors in `Graph`. The graph is therefore not a true over-approximation of the CFG — it under-approximates reachability in the presence of back-edges and join points, and so `elimMitigatedEdgesAndNodes` (which performs a DFS over `isCFGEdge`s in `Graph`) can incorrectly classify a sink as unreachable from a source, eliminating a real gadget edge.

The intended fix is the standard one: split into "visit body once" vs. "explore successors always," e.g.

```cpp
bool Inserted = BlocksVisited.insert(MBB).second;
if (Inserted) {
  // ... add intra-block nodes and terminator, advance GI ...
}
for (MachineBasicBlock *Succ : MBB->successors())
  TraverseCFG(Succ, GI, LoopDepth);
```

## Why it matters

If a source's CFG-reachable closure in `Graph` misses the actual sink (due to a dropped successor traversal on revisit), `elimMitigatedEdgesAndNodes` will treat the gadget as already mitigated, no LFENCE will be cut for that edge, and the LVI security guarantee is violated.

This is a **security-correctness bug** in a hardening pass, not a perf regression.

## Confidence

Medium. The pattern (early return after recording the entry edge but before recursing into successors) is unambiguous; whether a real CFG can be constructed that exposes a "lost" gadget via this path needs a concrete reproducer with a loop/diamond containing a SOURCE in one block and SINK in another such that the SINK is reachable from SOURCE only along a path that re-enters a previously-visited join block. The pass is gated on `useLVILoadHardening()`, so cross-checking with `clang -mlvi-hardening` is possible.
