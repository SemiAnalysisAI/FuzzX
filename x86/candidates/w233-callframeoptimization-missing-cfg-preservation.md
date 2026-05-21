# w233: X86CallFrameOptimizationPass::run forgets to preserve CFGAnalyses

## Summary

`X86CallFrameOptimization` rewrites MOV-to-stack-slot sequences into PUSH instructions *within a single basic block* — it never adds, removes, splits, or reorders MBBs. Therefore CFG analyses (MachineDominatorTree, MachineLoopInfo, BranchProbabilityInfo, ...) ARE preserved by this transformation. But the new-PM `X86CallFrameOptimizationPass::run` only returns `getMachineFunctionPassPreservedAnalyses()` without `.preserveSet<CFGAnalyses>()`.

Compare with every other in-MBB-only X86 pass in the same backend (`X86CompressEVEX`, `X86DomainReassignment`, `X86FixupInstTuning`, `X86FixupVectorConstants`, `X86OptimizeLEAs`, `X86CleanupLocalDynamicTLS`, `X86ArgumentStackSlotRebase`) — all of them call `.preserveSet<CFGAnalyses>()` after `getMachineFunctionPassPreservedAnalyses()`. `X86CallFrameOptimization` is the lone outlier.

The downstream consequence: every CFG-dependent analysis that the new-PM pass manager has cached is invalidated and recomputed by the next pass to query it. This isn't a wrong-code bug but is a silent compile-time regression every time this pass mutates a function.

## Source location

`llvm/lib/Target/X86/X86CallFrameOptimization.cpp` lines 643-650:

```c++
PreservedAnalyses
X86CallFrameOptimizationPass::run(MachineFunction &MF,
                                  MachineFunctionAnalysisManager &MFAM) {
  X86CallFrameOptimizationImpl Impl;
  bool Changed = Impl.runOnMachineFunction(MF);
  return Changed ? getMachineFunctionPassPreservedAnalyses()
                 : PreservedAnalyses::all();        // <<< missing .preserveSet<CFGAnalyses>()
}
```

Compare with `X86CompressEVEX.cpp` lines 526-535:

```c++
PreservedAnalyses
X86CompressEVEXPass::run(MachineFunction &MF, ...) {
  bool Changed = runOnMF(MF);
  if (!Changed) return PreservedAnalyses::all();
  PreservedAnalyses PA = getMachineFunctionPassPreservedAnalyses();
  PA.preserveSet<CFGAnalyses>();                    // <<< correct
  return PA;
}
```

## Fix

```diff
-  return Changed ? getMachineFunctionPassPreservedAnalyses()
-                 : PreservedAnalyses::all();
+  if (!Changed)
+    return PreservedAnalyses::all();
+  PreservedAnalyses PA = getMachineFunctionPassPreservedAnalyses();
+  PA.preserveSet<CFGAnalyses>();
+  return PA;
```

## Why this is a bug (despite no wrong asm)

CFG-dependent machine analyses (`MachineDominatorTree`, `MachineLoopInfo`, `MachineBlockFrequencyInfo`, ...) survive only when the modifying pass explicitly says it preserves the CFG. This pass does preserve the CFG (it only mutates instructions inside one MBB). By failing to declare that preservation:

1. Each time CallFrameOptimization fires (which it does any time stack-passing of arguments could be pushified), the next pass that requests, say, MachineDominatorTree will pay to rebuild it.
2. This is a real cost — `MachineDominatorTree::recalculate` is O(MBBs) per build and runs on every call site in some pipelines (e.g. `-O2` with `-fcf-protection=branch`).
3. The legacy-PM `runOnMachineFunction` is correct in not advertising preservation, because the legacy PM model doesn't have a "preserve" notion at this granularity. The new-PM port introduced the bug.
