# w601 — MachineLICM: review of atomic-barrier discipline (no-bug-found in this session)

## Target
- `llvm/lib/CodeGen/MachineLICM.cpp` — early MachineLICM (pre-RA), specifically:
  - `InitializeLoadsHoistableLoops()` lines 1450-1487
  - `IsLICMCandidate()` lines 1078-1110
  - `MachineInstr::isSafeToMove` in `MachineInstr.cpp:1350-1397`

## Mechanism reviewed

`InitializeLoadsHoistableLoops` walks every block of every loop and
marks the loop (and its ancestors) as not-allowed-to-hoist-loads iff
the block contains a load-fold barrier, a store, a call, or an ordered
load:

```
1477      for (auto &MI : *MBB) {
1478        if (!MI.isLoadFoldBarrier() && !MI.mayStore() && !MI.isCall() &&
1479            !(MI.mayLoad() && MI.hasOrderedMemoryRef()))
1480          continue;
1481        for (MachineLoop *L = Loop; L != nullptr; L = L->getParentLoop())
1482          AllowedToHoistLoads[L] = false;
1483        break;
1484      }
```

`MachineInstr::isLoadFoldBarrier()` is defined at `MachineInstr.cpp:1687`:

```
bool MachineInstr::isLoadFoldBarrier() const {
  return mayStore() || isCall() ||
         (hasUnmodeledSideEffects() && !isPseudoProbe());
}
```

That covers stores, calls, and unmodeled-side-effect instructions.
Ordered atomic loads (which have `mayLoad() && hasOrderedMemoryRef()`)
are added explicitly via the `MI.mayLoad() && MI.hasOrderedMemoryRef()`
clause.

`IsLICMCandidate` line 1080 sets
`DontMoveAcrossStore = !HoistConstLoads || !AllowedToHoistLoads[CurLoop]`.
With `HoistConstLoads=true` (default), `DontMoveAcrossStore=false` only
if the loop is on the allow-list.  When it is *not* on the allow-list,
`DontMoveAcrossStore=true` and `isSafeToMove(SawStore=true)` is
consulted, which conservatively bails out for loads when any store has
been observed.

## Empirical test (x86 default -O2)

`x86/candidates/w601-licm-atomic.ll`: an `!invariant.load` paired with
a `load atomic seq_cst` inside the loop.  At `early-machinelicm`:

```
$ llc -O2 -mtriple=x86_64-unknown-linux-gnu \
      -print-after=early-machinelicm w601-licm-atomic.ll

bb.1.loop:
  %9:gr64 = MOV64rm %4:gr64 :: (load seq_cst (s64) from %ir.sync)
  %10:gr64 = MOV64rm %5:gr64 :: (invariant load (s64) from %ir.p)
  ; store, add, etc.
```

The invariant load is NOT hoisted, as desired.  Same outcome with the
ordering changed to `acquire`.

## What I looked for but did not find

1. A loop with the atomic-ordered load in an *inner* MachineLoop while
   the hoist target is the *outer* MachineLoop's preheader.  The
   `for (MachineLoop *L = Loop; L != nullptr; L = L->getParentLoop())`
   walk at line 1481 propagates the not-allowed mark up the tree, and
   the visit order at line 1472 is "reverse pre-order DFS" (innermost
   first).  Both directions look correct.

2. A loop whose membership changes after `MachineLoopInfo` is built
   (e.g. block splitting in a later pass) — but MachineLICM runs early
   and rebuilds `MLI` if needed.

3. A pseudo-load with non-trivial side effects but with `mayLoad()`
   returning false (e.g. `INLINEASM` reading memory).  Inline asm has
   `hasUnmodeledSideEffects()` so `isLoadFoldBarrier()` catches it.

Conclusion for this lead: barrier discipline looks intact on the lines
inspected.  Filing as a documentation candidate, not a defect.

## Files
- `x86/candidates/w601-licm-atomic.ll`
