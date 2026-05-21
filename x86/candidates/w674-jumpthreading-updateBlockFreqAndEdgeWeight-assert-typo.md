# w674: JumpThreading `updateBlockFreqAndEdgeWeight` assertion uses `!BFI && !BFI` instead of `!BFI && !BPI`

## Pass
`-passes=jump-threading` (default x86 -O2 pipeline includes this).

## Summary

The first assertion of `updateBlockFreqAndEdgeWeight` is intended to
enforce that BFI and BPI are either both set or both unset. The check is
written as:

```cpp
assert(((BFI && BPI) || (!BFI && !BFI)) &&
       "Both BFI & BPI should either be set or unset");
```

The second clause `(!BFI && !BFI)` is a copy/paste typo — it tests `BFI`
twice and never checks `BPI`. The assertion therefore **never fires** for
the case it was specifically intended to catch: `BFI == nullptr,
BPI != nullptr`. The body of the function reads only `if (!BFI) { ... }`,
so a stray non-null `BPI` paired with a null `BFI` will silently flow
through the early return without ever being noticed in builds with
assertions enabled.

Trivially fixable — but a real bug in the safety net of the function, and
worth fixing for the same reason the assertion exists in the first place
(catching internal-callsite mistakes during refactors).

## Source (LLVM 23.0.0git, `llvm/lib/Transforms/Scalar/JumpThreading.cpp`)

```cpp
// JumpThreading.cpp:2528-2542
void JumpThreadingPass::updateBlockFreqAndEdgeWeight(BasicBlock *PredBB,
                                                     BasicBlock *BB,
                                                     BasicBlock *NewBB,
                                                     BasicBlock *SuccBB,
                                                     BlockFrequencyInfo *BFI,
                                                     BranchProbabilityInfo *BPI,
                                                     bool HasProfile) {
  assert(((BFI && BPI) || (!BFI && !BFI)) &&     // <-- typo: should be !BPI
         "Both BFI & BPI should either be set or unset");

  if (!BFI) {
    assert(!HasProfile &&
           "It's expected to have BFI/BPI when profile info exists");
    return;
  }
  ...
}
```

`git blame` confirms the typo has been there since the function was
introduced. Other callsites that pass BFI/BPI always pass them in lockstep
via the `getOrCreateBFI` / `getOrCreateBPI` helpers — so the broken
assertion is dormant in normal usage, but it is still incorrect.

## Reproducer

Static-code observation, no IR repro required. The fix is mechanical:

```diff
-  assert(((BFI && BPI) || (!BFI && !BFI)) &&
+  assert(((BFI && BPI) || (!BFI && !BPI)) &&
           "Both BFI & BPI should either be set or unset");
```

## Why this matters

- Defensive assertions exist to catch caller mistakes. A typo that
  short-circuits the check defeats its purpose.
- A future refactor that passes BPI without BFI (or vice versa) will not
  be caught at the entry of this function — instead it will silently
  return at `if (!BFI)` (because BFI is null), leaving the BPI updates
  that the caller may have expected to happen un-applied.
- This is exactly the class of bug that LLVM's assertion convention is
  designed to surface.

## Suggested fix

One-character fix: replace the second `!BFI` with `!BPI`.

## Related

- This is the same function that performs the post-threadEdge BFI/BPI
  updates discussed in w672/w673; if those bugs are revisited, this typo
  should be fixed at the same time.
