# w343: MachineInstrExpressionTrait::getHashValue ignores MMOs, hash-collides loads that intentionally differ

## Summary
`MachineInstrExpressionTrait::getHashValue` (used as the DenseMap hash by
MachineCSE's `ScopedHashTable`) hashes opcode + non-vreg-def MachineOperands
ONLY. It does not include `memoperands()`. Combined with `isIdenticalTo`
(which is the DenseMap equality predicate for the same trait) ALSO ignoring
MMOs, this means MachineCSE's `VNT` map treats every load from the same
address as a single bucket regardless of MMO differences.

This is the root cause that w340 (MachineCSE drops MMOs) and w341 (LICM
EliminateCSE drops MMOs) inherit. Filing separately because the hash function
is shared infrastructure used by *every* future place that wants
"value-numbered MIs" to mean "MIs that produce equivalent value", and an
extra-cautious site that *does* try to merge MMOs would still mis-bucket
its inputs because they all collide into one VN.

## Root cause (source citations)

`llvm/lib/CodeGen/MachineInstr.cpp:2332-2345`:
```cpp
unsigned
MachineInstrExpressionTrait::getHashValue(const MachineInstr* const &MI) {
  SmallVector<size_t, 16> HashComponents;
  HashComponents.reserve(MI->getNumOperands() + 1);
  HashComponents.push_back(MI->getOpcode());
  for (const MachineOperand &MO : MI->operands()) {
    if (MO.isReg() && MO.isDef() && MO.getReg().isVirtual())
      continue;  // Skip virtual register defs.
    HashComponents.push_back(hash_value(MO));
  }
  return hash_combine_range(HashComponents);
}
```

No iteration over `MI->memoperands()`.

`llvm/include/llvm/CodeGen/MachineInstr.h:2142-2148`:
```cpp
struct MachineInstrExpressionTrait : DenseMapInfo<MachineInstr*> {
  ...
  static bool isEqual(const MachineInstr* const &LHS,
                      const MachineInstr* const &RHS) {
    ...
    return LHS->isIdenticalTo(*RHS, MachineInstr::IgnoreVRegDefs);
  }
};
```

`isIdenticalTo` does not look at MMOs either (`MachineInstr.cpp:673-740`).

## Use sites
`grep -rn "MachineInstrExpressionTrait" llvm/lib/CodeGen/`:
- `MachineCSE.cpp` `VNT` / `Exps` / `PREMap`
- `MachineLICM.cpp` `CSEMap` (via opcode keys + LookForDuplicate)
- `MIRCanonicalizerPass.cpp` for renaming
- `EarlyIfConversion.cpp` via DenseSet of MIs

Every site that uses this trait treats two MIs with different MMOs as
indistinguishable for value-numbering. Some sites (`BranchFolding.cpp:822`)
remember to call `cloneMergedMemRefs` afterwards. MachineCSE and
MachineLICM::EliminateCSE do not.

## Fix sketch
Two options, in increasing order of churn:

1. Have `MachineInstrExpressionTrait` mix `MI->memoperands().data()` pointer
   identity into the hash and have `isEqual` require `memoperands_size() ==`
   and pointer-identical MMO arrays. This excludes intentionally-CSE-able
   pairs (which is fine; MMOs that compare equal-by-value will still have
   different pointers, but the cases that matter -- e.g. two different load
   instances with `!range` -- almost always have differently-allocated MMOs).
   Downside: fewer CSE opportunities.

2. Keep the trait permissive, but require every CSE callsite to either
   (a) call `cloneMergedMemRefs({CSMI, MI})` before erasing MI, or
   (b) call `MMOsHaveSameValue(CSMI, MI)` and bail out if not.

LLVM has both helpers already (`MachineMemOperand::operator==`,
`MachineInstr::cloneMergedMemRefs`). The fix at the trait level is one place
to change; the fix at each CSE site is many.

## Cite
- `llvm/lib/CodeGen/MachineInstr.cpp:2332-2345`
  (`MachineInstrExpressionTrait::getHashValue`)
- `llvm/include/llvm/CodeGen/MachineInstr.h:2140-2150`
  (`MachineInstrExpressionTrait::isEqual` -> `isIdenticalTo`)
- `llvm/lib/CodeGen/MachineInstr.cpp:673-740` (`isIdenticalTo` ignores MMOs)
- `llvm/lib/CodeGen/MachineInstr.cpp:429-478` (`cloneMergedMemRefs` exists)
- `llvm/lib/CodeGen/BranchFolding.cpp:822` (a place that actually calls
  `cloneMergedMemRefs` after merging two MIs -- the "right" pattern)
