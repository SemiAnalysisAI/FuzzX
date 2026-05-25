# Root-Cause Patches

S6 in TRIAGE.md is ~85 metadata-loss bugs. They collapse to **7 root-cause patches** in shared helpers, each closing many catalog entries. Patches below are against LLVM 23.0.0git at the FuzzX-pinned tree (HEAD ≈ `0dd29960cd61`); line numbers may shift slightly on rebase.

---

## Patch A — `combineMetadata` cleanup (closes ~10 bugs)

**File:** `llvm/lib/Transforms/Utils/Local.cpp`, function `combineMetadata` (lines ~2932-3110).

Three independent defects:

### A1. `MD_nontemporal` arm strips K's tag when J lacks it (#229)

Current:
```cpp
case LLVMContext::MD_nontemporal:
  // Preserve !nontemporal if it is present on both instructions.
  if (!AAOnly)
    K->setMetadata(Kind, JMD);                    // <-- if JMD is null, this clears K's tag
  break;
```

The comment ("Preserve …if present on both") is wrong — the code clears K's tag whenever J lacks it, even when K is the stationary CSE leader. Compare `MD_invariant_load` which correctly guards on `DoesKMove`.

Fix:
```cpp
case LLVMContext::MD_nontemporal:
  // If K moves, !nontemporal must be on both instructions.
  // If K is stationary, K's tag survives.
  if (!AAOnly && DoesKMove)
    K->setMetadata(Kind, JMD);
  break;
```

### A2. `MD_nosanitize` arm strips K's tag when J lacks it (#230)

Current:
```cpp
case LLVMContext::MD_nosanitize:
  // Preserve !nosanitize if both K and J have it.
  K->setMetadata(Kind, JMD);                      // <-- same bug as A1
  break;
```

Fix:
```cpp
case LLVMContext::MD_nosanitize:
  if (DoesKMove)
    K->setMetadata(Kind, JMD);
  break;
```

### A3. Loop iterates only K's metadata; J-only kinds silently dropped (#219, #447)

Current:
```cpp
SmallVector<std::pair<unsigned, MDNode *>, 4> Metadata;
K->getAllMetadataOtherThanDebugLoc(Metadata);
for (const auto &MD : Metadata) {
  unsigned Kind = MD.first;
  ...
}
```

After this loop the function special-cases `MD_invariant_group`, `MD_mmra`, `MD_memprof`, `MD_callsite`, `MD_prof` to handle "J has it but K doesn't." Everything else (`MD_tbaa`, `MD_alias_scope`, `MD_noalias`, `MD_range`, `MD_nontemporal`, `MD_nonnull`, …) is silently lost when J carries the kind and K doesn't.

Fix: walk the union of K's and J's metadata kinds:
```cpp
SmallVector<std::pair<unsigned, MDNode *>, 4> KMetadata, JMetadata;
K->getAllMetadataOtherThanDebugLoc(KMetadata);
J->getAllMetadataOtherThanDebugLoc(JMetadata);
SmallSet<unsigned, 8> SeenKinds;
for (const auto &MD : KMetadata) SeenKinds.insert(MD.first);
for (const auto &MD : JMetadata)
  if (SeenKinds.insert(MD.first).second)
    KMetadata.push_back({MD.first, nullptr});  // K-side absent
// then iterate KMetadata as today (KMD may be null for J-only kinds)
```

Or, simpler: keep the loop as is but add a post-pass that iterates `JMetadata` looking for any kind not in `SeenKinds` and applies the per-kind merge rule (intersect, getMostGenericRange, etc.) — bias toward the conservative direction (drop the kind) when in doubt.

### Bugs closed by Patch A

#219, #229, #230, #287 (=#229 duplicate sibling), #288 (=#230 sibling), #447 (J-only TBAA dropped). Additionally cleans up the in-tree FIXME at line ~3063 for `MD_invariant_group`.

---

## Patch B — `MachineInstr::isIdenticalTo` should optionally compare MMOs (closes 5 bugs)

**File:** `llvm/lib/CodeGen/MachineInstr.cpp`, function `MachineInstr::isIdenticalTo` (lines ~673-740) + `MachineInstrExpressionTrait::getHashValue`/`isEqual` (lines ~2332-2345).

Two MIs with identical opcode/operands but **different MMOs** (e.g., one `load atomic monotonic`, one plain `load`) currently compare equal. Sites that erase the loser based on this equality silently lose the loser's MMO info (atomic ordering, syncscope, `!nontemporal`, `!invariant.load`, `!range`).

Fix: add a new `MICheckType::Strict` (or extend the existing enum) that also requires `hasIdenticalMMOs(*A, *B)`:

```cpp
bool MachineInstr::isIdenticalTo(const MachineInstr &Other,
                                 MICheckType Check) const {
  // ... existing opcode/operand checks ...

  // For Strict comparison, also require identical MMOs.
  if (Check == StrictIncludingMMOs && !hasIdenticalMMOs(*this, Other))
    return false;

  return true;
}
```

And update `MachineInstrExpressionTrait::getHashValue` to include MMO hashes (or be conservative and refuse to hash MIs with non-empty MMOs).

Call-site fixes:
- `MachineCSE.cpp` `ProcessBlockCSE`: before `MI.eraseFromParent()`, call `CSMI->cloneMergedMemRefs(*MF, {CSMI, &MI})` to combine MMOs (mirror BranchFolder pattern).
- `MachineLateInstrsCleanup.cpp` `Reg2MIMap::hasIdentical`: pass `StrictIncludingMMOs`.
- `BranchFolding.cpp` `mergeCommonTails` and `HoistCommonCodeInSuccs`: use `cloneMergedMemRefs` (already done in some sites but not others).

### Bugs closed by Patch B

#141, #237 (MachineCSE), #239 (MachineLateInstrsCleanup), #357 (BranchFolder pcsections), w340 candidate. Also fixes the in-source comment at `MachineScheduler.cpp:2148-2154` re cluster-mode bypass.

---

## Patch C — `MachineMemOperand::operator==` should compare ordering + syncscope (closes 4 bugs)

**File:** `llvm/include/llvm/CodeGen/MachineMemOperand.h` lines ~349-360.

Current:
```cpp
friend bool operator==(const MachineMemOperand &LHS,
                       const MachineMemOperand &RHS) {
  return LHS.getValue() == RHS.getValue() &&
         LHS.getPseudoValue() == RHS.getPseudoValue() &&
         LHS.getSize() == RHS.getSize() &&
         LHS.getOffset() == RHS.getOffset() &&
         LHS.getFlags() == RHS.getFlags() &&
         LHS.getAAInfo() == RHS.getAAInfo() &&
         LHS.getRanges() == RHS.getRanges() &&
         LHS.getAlign() == RHS.getAlign() &&
         LHS.getAddrSpace() == RHS.getAddrSpace();
}
```

Missing: `getSuccessOrdering()`, `getFailureOrdering()`, `getSyncScopeID()`.

Fix:
```cpp
friend bool operator==(const MachineMemOperand &LHS,
                       const MachineMemOperand &RHS) {
  return LHS.getValue() == RHS.getValue() &&
         LHS.getPseudoValue() == RHS.getPseudoValue() &&
         LHS.getSize() == RHS.getSize() &&
         LHS.getOffset() == RHS.getOffset() &&
         LHS.getFlags() == RHS.getFlags() &&
         LHS.getAAInfo() == RHS.getAAInfo() &&
         LHS.getRanges() == RHS.getRanges() &&
         LHS.getAlign() == RHS.getAlign() &&
         LHS.getAddrSpace() == RHS.getAddrSpace() &&
         LHS.getSuccessOrdering() == RHS.getSuccessOrdering() &&
         LHS.getFailureOrdering() == RHS.getFailureOrdering() &&
         LHS.getSyncScopeID() == RHS.getSyncScopeID();
}
```

The downstream `MachineInstr::hasIdenticalMMOs` already iterates each MMO and uses this `operator==`, so a single fix cascades.

### Bugs closed by Patch C

#226 (BranchFolder atomic ordering drop), #238 (BranchFolder syncscope narrow), #355, #356 (BranchFolder family).

---

## Patch D — Wide DAGCombiner `getLoad`/`getStore` overloads (closes 9 bugs)

**File:** `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp` (multiple sites) and `llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp` (helpers `getMemcpyLoadsAndStores`, `getMemmoveLoadsAndStores`, `getMemsetStores`).

All sites use the 4-arg `getLoad(VT, dl, Chain, Ptr, PtrInfo, Align, MMOFlags)` / `getStore(...)` overload, which **defaults `AAMDNodes` to empty and ignores source MMO flags beyond `isVol`**. The 6-arg overload preserves both.

### D1. Per-site call-pattern fix

At each affected site, replace the 4-arg overload with the 6-arg form that takes `MachineMemOperand::Flags MMOFlags, const AAMDNodes &AAInfo, const MDNode *Ranges = nullptr`, and forward both the source MMOs' flags and AAInfo (combined via intersection where both source MMOs are available).

Affected sites and the bug each closes:
| Bug | Site (DAGCombiner.cpp / SelectionDAG.cpp) | Function |
|-----|-------------------------------------------|----------|
| #196 | `tryStoreMergeOfLoads` ~23590-23625 | both `getLoad` and `getStore` |
| #197 | `mergeTruncStores` ~9929-9931 | `getStore` |
| #198 | `ReduceLoadOpStoreWidth` ~22441-22450 | `getStore` (asymmetric; load already correct) |
| #199 | `CombineConsecutiveLoads` ~17581-17582 | `getLoad` |
| #208 | `getMemcpyLoadsAndStores` ~9331-9332 | both |
| #209 | `getMemmoveLoadsAndStores` ~9520-9521 | both |
| #210 | `getMemsetStores` ~9747-9751 | `getStore` |
| #224 | SDAGBuilder `visitAtomicRMW`/`visitAtomicCmpXchg` 5213/5285 | `getMachineMemOperand` — use `I.getAlign()` not `getEVTAlign(MemVT)`, and `I.getAAMetadata()` |

### D2. Mem-intrinsic NT propagation (the SDAG side of the `!nontemporal` story)

`SelectionDAGBuilder.cpp` `visitMemCpyInst` / `visitMemMoveInst` / `visitMemSetInst` (~lines 6695-6736) currently read `align`/`isVol` from the call but never query `MD_nontemporal`. The helpers' `isVol` should be augmented to a `MachineMemOperand::Flags` parameter; the SDAGBuilder side should OR in `MONonTemporal` when the intrinsic carries `!nontemporal`.

### Bugs closed by Patch D

#196–#199, #208–#210, #224, plus dovetails into #140 (CGP splitMergedValStore, same root pattern in a different pass).

---

## Patch E — `ScalarizeMaskedMemIntrin` per-lane `copyMetadata` (closes 5 bugs)

**File:** `llvm/lib/Transforms/Scalar/ScalarizeMaskedMemIntrin.cpp`.

The whole file has 4 `copyMetadata` calls, all for the constant-mask all-true short-cut of `scalarizeMaskedLoad`/`Store` (lines 167, 211, 339, 376). All other per-lane load/store creation sites lack the call:

| Bug | Function | Per-lane site |
|-----|----------|---------------|
| #202 | `scalarizeMaskedGather` (dynamic-mask) | line 558 — after `Builder.CreateAlignedLoad(...)`, missing `Load->copyMetadata(*CI)` |
| #203 | `scalarizeMaskedScatter` (dynamic-mask) | line 692 — after `Builder.CreateAlignedStore(...)`, missing `Store->copyMetadata(*CI)` |
| #204 | `scalarizeMaskedExpandLoad` (both const+dyn paths) | lines 748, 806 — same |
| #205 | `scalarizeMaskedCompressStore` (both const+dyn paths) | lines 877, 927 — same |
| #180 | `scalarizeMaskedGather/Scatter` constant-mask fast paths | lines 184-194, 350-359, 493-506, 631-642 — same |

Single-PR fix: walk the 7 per-lane create sites listed above and append:
```cpp
NewI->copyMetadata(*CI);
```

This is correctness-neutral for AA metadata (per-lane copy is the right semantics for gather/scatter/expand/compress).

### Bugs closed by Patch E

#180, #202, #203, #204, #205.

---

## Patch F — `dropUBImplyingAttrsAndMetadata` keep-list (closes ~6 bugs)

**File:** `llvm/lib/IR/Instruction.cpp` lines ~586-602 (`dropUBImplyingAttrsAndMetadata` keep-list).

The function's "keep" list is `MD_annotation, MD_range, MD_nonnull, MD_align, MD_fpmath, MD_prof`. It drops `MD_tbaa, MD_nontemporal, MD_invariant_load, MD_invariant_group, MD_access_group, MD_mem_parallel_loop_access, MD_memprof, MD_callees, MD_callee_type, MD_callsite` — none of which are UB-implying (they are AA/performance hints).

Callers in SimplifyCFG (`speculativelyExecuteBB` line 3386, `hoistAllInstructionsInto` for `foldTwoEntryPHINode` at Local.cpp:3421) lose all these kinds on speculated/hoisted instructions.

Fix: split the keep-list into two:
- **UB-implying kinds to drop**: `noundef`, `range`, `nonnull`, `align`, `dereferenceable`, `dereferenceable_or_null` (these can introduce immediate UB if violated after speculation).
- **Non-UB-implying kinds to KEEP across speculation**: everything else (`tbaa`, `nontemporal`, `invariant.load`, `invariant.group`, `access_group`, `mem_parallel_loop_access`, `memprof`, `callees`, `callee_type`, `callsite`, `noalias_addrspace`, `nosanitize`, etc.).

The current code accidentally drops the second group.

### Bugs closed by Patch F

#091 (SimplifyCFG hoistCondLoads), #183 (SimplifyCFG hoist memintrinsic), #420 (speculativelyExecuteBB), #421 (foldTwoEntryPHINode hoistAllInstructionsInto), #496 (simplifycfg speculate store NT), #498 (instcombine unpack aggregate NT).

---

## Patch G — `JumpThreading` should forward `MD_unpredictable`/`MD_annotation` (closes 5 bugs)

**File:** `llvm/lib/Transforms/Scalar/JumpThreading.cpp`.

The entire file never references `MD_unpredictable`. Every place that builds a new conditional branch from a `select` only forwards `MD_prof`:

- `unfoldSelectInstr` ~line 2794: `BI->copyMetadata(*SI, {LLVMContext::MD_prof})`
- `tryToUnfoldSelectInCurrBB` ~line 2990: only `MD_prof` to `SplitBlockAndInsertIfThen`

Fix: change both to:
```cpp
BI->copyMetadata(*SI, {LLVMContext::MD_prof,
                       LLVMContext::MD_unpredictable,
                       LLVMContext::MD_annotation});
```

Same shape applies to `duplicateCondBranchOnPHIIntoPred` (`!prof` no scaling — see #672, separate issue).

### Bugs closed by Patch G

#214, #260, #261, #263, #672 (candidate), plus dovetails into SimplifyCFG bugs (#424, #646, #647, #648).

---

## Summary: bugs closed by the 7 root-cause patches

| Patch | Bugs closed | Lines of code |
|-------|-------------|---------------|
| A — combineMetadata (Local.cpp) | #219, #229, #230, #287, #288, #447 + FIXME line 3063 | ~30 lines |
| B — MachineInstr::isIdenticalTo MMO (MachineInstr.cpp + 4 callers) | #141, #237, #239, #357, w340 | ~50 lines (incl. callers) |
| C — MachineMemOperand::operator== (MachineMemOperand.h) | #226, #238, #355, #356 | 3 lines |
| D — DAGCombiner 4-arg overloads (DAGCombiner.cpp, SelectionDAG.cpp) | #196–#199, #208–#210, #224 (8) | ~120 lines across 8 sites |
| E — ScalarizeMaskedMemIntrin (ScalarizeMaskedMemIntrin.cpp) | #180, #202, #203, #204, #205 | 7 lines (one per site) |
| F — dropUBImplyingAttrsAndMetadata (Instruction.cpp) | #091, #183, #420, #421, #496, #498 | ~10 lines |
| G — JumpThreading unpredictable/annotation forwarding | #214, #260, #261, #263, #672 | ~6 lines |
| **TOTAL** | **~40 bugs closed by 7 PRs** | **~225 lines** |

Plus, on top of this, the SROA family (Patch H, ~9 bugs) and LICM promoteLoopAccessesToScalars syncscope tracking (Patch I, ~6 bugs) would mop up another ~15 bugs each — both are larger patches (~50 lines) because they need to track new state through helpers.

---

## Suggested upstream PR sequencing

1. **Patch C first** (3 lines, mechanical, no behavior change for non-atomic MIR) — closes 4 bugs, unblocks B.
2. **Patch B** (now that C is in, the MMO compare is correct) — closes 5 bugs.
3. **Patch E** (7 one-line additions, completely mechanical) — closes 5 bugs.
4. **Patch A** (combineMetadata cleanup; touches a hot helper, will need careful review) — closes ~7 bugs.
5. **Patch G** (JumpThreading metadata) — closes 5 bugs.
6. **Patch F** (Instruction.cpp keep-list split; semantic change, will need RFC) — closes ~6 bugs.
7. **Patch D** (DAGCombiner; largest but most impactful; possibly split into 3 sub-PRs by file) — closes 8+ bugs.

Once these land, the catalog's S6 tier shrinks from ~85 entries to ~45 entries (the long tail of pass-specific copy sites: SROA, LICM, MemCpyOpt, GVN PRE, etc.). Those remain pass-by-pass fixes.
