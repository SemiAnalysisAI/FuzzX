# m119: `SIMemOpAccess::constructFromMIWithMMO` SSID merge silently drops the stronger scope when MMOs aren't comparable

*Discovery method: code inspection.*  Sibling shape to m119 family of
target-side ordering bugs.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIMemoryLegalizer.cpp:836`:

```cpp
SSID = *IsSyncScopeInclusion ? SSID : MMO->getSyncScopeID();
```

The merge only checks `isSyncScopeInclusion(A, B)`.  When neither
subsumes the other (e.g. `A = agent-one-as`, `B = workgroup` cross-AS,
which live in disjoint partial orders), the optional returns `false`
(not `nullopt`), and the code blindly **overwrites `SSID := B`**,
silently dropping the agent-level scope.

Consequence: for a MachineInstr that carries multiple MMOs with
distinct SSIDs (created by MIR-level memref merging via
`cloneMergedMemRefs` at `MachineInstr.cpp:429`, or by target
lowering's `setNodeMemRefs` with a merged list), the resulting SSID
depends on **MMO order**, not on the LUB of the two scopes.

## Reproducer

`reduced.mir` (in this dir) builds a `FLAT_ATOMIC_ADD` with two MMOs:

* `(load store syncscope("agent-one-as") seq_cst (s32))`
* `(load store syncscope("workgroup") seq_cst (s32))`

`llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 -run-pass=si-memory-legalizer reduced.mir`:

* First-MMO-agent / second-MMO-workgroup ordering: only LDS waitcnt
  emitted.  **No `BUFFER_WBL2 16` / `BUFFER_INV 16`** -- AGENT writeback
  and invalidate dropped.
* Reversed ordering: emits both `BUFFER_WBL2 16` and `BUFFER_INV 16`
  around the atomic, plus the LDS waitcnt.

Same input, codegen differs by MMO order.  In the first case, the
AGENT cache-management ops are silently absent -- an L2 read from a
sibling agent's cache could return stale data after this atomic
completes.

## Why this matters

* SDAG rarely emits multi-MMO atomics directly -- but MIR-level passes
  do (`MachineInstr::cloneMergedMemRefs` is called by load/store
  combiners, scheduler hoisting, and target peepholes).
* Any future enhancement that exposes the merge to a fuzz-shape
  (e.g., a target combine that hoists an atomic with merged refs) will
  trip this immediately.
* The behavior is also non-deterministic across LLVM revisions because
  MMO ordering depends on attachment site, which is opaque to source.

## Suggested fix

Compute the LUB of the two SSIDs explicitly.  For AMDGPU's 12 SSIDs,
this is a small lookup:

```cpp
// Pseudocode:
SyncScope::ID MergedSSID = ssidLUB(A_SSID, B_SSID);
if (MergedSSID == SyncScope::ID::Invalid)
  return reportUnsupported(...);
SSID = MergedSSID;
```

Or fall back to the strictly-larger of (system > agent > workgroup >
wavefront > singlethread) plus "cross-AS dominates one-AS".

## Related: cross-AS SSID with FLAT MMO never widens OrderingAddrSpace

`SIMemoryLegalizer.cpp:756-774`: one-AS SSIDs compute
`OrderingAddrSpace = ATOMIC & InstrAddrSpace`.  For a
`syncscope("agent-one-as")` MMO with FLAT pointer (which can alias
GLOBAL / LDS / SCRATCH), the "single AS" guarantee is violated by the
IR yet the legalizer happily emits
`IsCrossAddressSpaceOrdering=false` and skips inter-AS ordering.  The
IR verifier should reject this combination, or `toSIAtomicScope`
should fall back to cross-AS when `InstrAddrSpace` spans multiple
ASes.  Separate fix, same family.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/llc`) | Same merge logic, same bug. |
