# w469: `llvm::scaleProfData` BranchWeights branch keeps only the first weight, silently dropping the remaining weights on multi-successor !prof

File: `llvm/lib/IR/ProfDataUtils.cpp`
Lines: 356-407 (focus 377-387)

## Summary

`scaleProfData(Instruction &I, uint64_t S, uint64_t T)` rebuilds the
`!prof` metadata after scaling counts by `S/T`. For value-profile metadata
it loops over every `(value, count)` pair (lines 388-406). For
**branch_weights** metadata, however, it only reads **one** weight — the
one at `getBranchWeightOffset(ProfileData)` — and pushes a single new
operand:

```cpp
if (ProfDataName->getString() == MDProfLabels::BranchWeights &&
    ProfileData->getNumOperands() > 0) {
  // Using APInt::div may be expensive, but most cases should fit 64 bits.
  APInt Val(128,
            mdconst::dyn_extract<ConstantInt>(
                ProfileData->getOperand(getBranchWeightOffset(ProfileData)))
                ->getValue()
                .getZExtValue());
  Val *= APS;
  Vals.push_back(MDB.createConstant(ConstantInt::get(
      Type::getInt32Ty(C), Val.udiv(APT).getLimitedValue(UINT32_MAX))));
}
```

The resulting `MDNode` therefore contains only `{tag (optional origin),
first-weight}` — every additional successor's weight is silently dropped.

`scaleProfData` is invoked from:

* `CallInst::updateProfWeight` (`Instructions.cpp:833`)
* `InvokeInst::updateProfWeight` (`Instructions.cpp:913`)
* `InlineFunction.cpp:2141` (vtable load updateVTableProfWeight, value
  profile only)

The intended use is on call/invoke `!prof` metadata, which is typically a
VP node, and the VP path is correct. But `InvokeInst` is *also* allowed to
carry a 2-element `branch_weights` (normal / unwind) — the verifier
(`Verifier.cpp:5392-5394`) explicitly accepts `NumBranchWeights == 1 ||
NumBranchWeights == 2`. When `updateProfWeight` is called for an invoke
whose `!prof` is `branch_weights` with two operands (e.g. after inlining
copies an invoke with a branch-weights annotation, the
`updateProfWeight(CloneEntryCount, PriorEntryCount)` call at
`InlineFunction.cpp:2150`), the rebuilt metadata loses the unwind weight
silently.

The post-scale node still verifies (the invoke "NumBranchWeights == 1"
branch is accepted), so this is a silent unwind-probability dropout
across inlining of any invoke with annotated unwind weights, and it skews
post-inline branch-probability inference (BPI/BFI treats the missing
weight as 0, i.e. dead-unwind).

## Citation

```cpp
// ProfDataUtils.cpp:377-387
if (ProfDataName->getString() == MDProfLabels::BranchWeights &&
    ProfileData->getNumOperands() > 0) {
  // Using APInt::div may be expensive, but most cases should fit 64 bits.
  APInt Val(128,
            mdconst::dyn_extract<ConstantInt>(
                ProfileData->getOperand(getBranchWeightOffset(ProfileData)))
                ->getValue()
                .getZExtValue());
  Val *= APS;
  Vals.push_back(MDB.createConstant(ConstantInt::get(
      Type::getInt32Ty(C), Val.udiv(APT).getLimitedValue(UINT32_MAX))));
}
```

vs the symmetric VP loop:
```cpp
// ProfDataUtils.cpp:388-406
} else if (ProfDataName->getString() == MDProfLabels::ValueProfile)
  for (unsigned Idx = 1; Idx < ProfileData->getNumOperands(); Idx += 2) {
    ...
  }
```

Note the asymmetry: VP iterates over **all** operand pairs, the
branch_weights branch reads a **single** operand. The fix mirrors the VP
loop: iterate `getBranchWeightOffset(ProfileData) .. NumOperands` and
push a scaled `Type::getInt32Ty` value for each.

## Why it's a bug pattern match

"!prof scaling overflow / wrong fallback" — when the inliner copies an
invoke with `!prof !{!"branch_weights", T, F}` and calls
`InvokeInst::updateProfWeight(S, T_total)`, the unwind weight (`F`) is
silently dropped from the cloned invoke's metadata, the Verifier still
accepts it (1-weight invokes are legal), and subsequent BPI/BFI runs see
unwind-probability = 0 for an exception path that may in fact be hot.
