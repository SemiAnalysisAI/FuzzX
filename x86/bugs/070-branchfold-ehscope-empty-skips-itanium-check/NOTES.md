# BranchFolding ProfitableToMerge skips EH-scope check when EHScopeMembership is empty

File: llvm/lib/CodeGen/BranchFolding.cpp, function `ProfitableToMerge`, lines 581-589.

```cpp
// It is never profitable to tail-merge blocks from two different EH scopes.
if (!EHScopeMembership.empty()) {
  auto EHScope1 = EHScopeMembership.find(MBB1);
  assert(EHScope1 != EHScopeMembership.end());
  auto EHScope2 = EHScopeMembership.find(MBB2);
  assert(EHScope2 != EHScopeMembership.end());
  if (EHScope1->second != EHScope2->second)
    return false;
}
```

`EHScopeMembership` is populated by `getEHScopeMembership(MF)`
(`llvm/lib/CodeGen/Analysis.cpp:757-799`), which early-returns an empty map
when `MF.hasEHScopes()` is false. `hasEHScopes()` is only set for
Windows/funclet-based EH personalities (SEH, MSVC CXX). For Itanium DWARF EH
(the default Linux x86_64 target!), this map is always empty even when the
function has multiple `invoke`s with distinct landing pads.

When the map is empty the check is silently skipped, so two tail blocks
belonging to different `invoke`-protected regions can be tail-merged. The
merged tail then contains a single `call` instruction whose LSDA call-site
record can only point to one landing pad; the second invoke's unwind action
is silently lost.

## Mitigating factor

Each `invoke` lowers with `EH_LABEL begin / call / EH_LABEL end` pseudo
instructions in the predecessor MBB. `countsAsInstruction` treats `EH_LABEL`
as a real instruction (it's neither debug nor CFI), and `isIdenticalTo` on
two `EH_LABEL`s with different `MCSymbol` operands returns false, so
`ComputeCommonTailLength` typically bails before crossing an EH_LABEL pair.

The bug surfaces when one of the merge candidates is the **trailing call**
of an invoke region (so the `EH_LABEL end` is at the BB boundary, not inside
the tail) and the other is a `call` to the same function outside any invoke
region. Both blocks end with identical `call` + `jmp continuation` sequences,
but their unwind semantics differ.

## Suggested fix

Compute and consult an EH-aware partition for Itanium DWARF too — e.g. assign
each MBB the call-site-table index of its enclosing invoke region, and refuse
to merge across different indices.

## Confidence

Source-level reasoning; need an Itanium-EH IR repro with two near-identical
invoke tails to confirm the merge actually fires. Filed for x86_64-linux-gnu
audit.
