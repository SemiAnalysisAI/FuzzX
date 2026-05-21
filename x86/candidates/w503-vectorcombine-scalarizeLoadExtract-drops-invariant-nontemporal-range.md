# w503 - VectorCombine `scalarizeLoadExtract` drops `!invariant.load`, `!nontemporal`, `!range` on scalar replacement loads

## Location

`llvm/lib/Transforms/Vectorize/VectorCombine.cpp`

- Entry: `VectorCombine::scalarizeLoadExtract` line 2063
- Defective creation: line 2129-2142

```cpp
// line 2129
auto *NewLoad = cast<LoadInst>(
    Builder.CreateLoad(ElemType, GEP, EI->getName() + ".scalar"));

Align ScalarOpAlignment =
    computeAlignmentAfterScalarization(LI->getAlign(), ElemType, Idx, *DL);
NewLoad->setAlignment(ScalarOpAlignment);

if (auto *ConstIdx = dyn_cast<ConstantInt>(Idx)) {
  size_t Offset = ConstIdx->getZExtValue() * DL->getTypeStoreSize(ElemType);
  AAMDNodes OldAAMD = LI->getAAMetadata();
  NewLoad->setAAMetadata(OldAAMD.adjustForAccess(Offset, ElemType, *DL));
}
```

Only alignment and AAMetadata (and only when the index is a constant)
are transferred. **All other load-carrying metadata is silently
discarded**, including `!invariant.load`, `!nontemporal`, `!range`,
`!noundef`, `!align`, `!dereferenceable`, `!dereferenceable_or_null`,
`!annotation`.

This is the same defect class as bugs w63b / w100 / w502 but for the
load-extract path: that path was patched for atomicity (w63b) and for AA
metadata, but other load-level hints were never wired up.

## Repro 1 — `!invariant.load` dropped

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define i32 @sle_inv(ptr %p) {
  %v = load <4 x i32>, ptr %p, align 16, !invariant.load !0
  %e0 = extractelement <4 x i32> %v, i32 0
  %e1 = extractelement <4 x i32> %v, i32 1
  %s  = add i32 %e0, %e1
  ret i32 %s
}
!0 = !{}
```

```
opt -mtriple=x86_64-unknown-linux-gnu -passes=vector-combine -S
```

Output:

```llvm
define i32 @sle_inv(ptr %p) {
  %e0 = load i32, ptr %p, align 16              ; <-- !invariant.load gone
  %1  = getelementptr inbounds <4 x i32>, ptr %p, i32 0, i32 1
  %e1 = load i32, ptr %1, align 4               ; <-- !invariant.load gone
  %s  = add i32 %e0, %e1
  ret i32 %s
}
```

Reproduces identically at `-O2`:

```llvm
define i32 @sle_inv(ptr readonly captures(none) %p) local_unnamed_addr {
  %e0 = load i32, ptr %p, align 16
  %1 = getelementptr inbounds nuw i8, ptr %p, i64 4
  %e1 = load i32, ptr %1, align 4
  %s = add i32 %e0, %e1
  ret i32 %s
}
```

## Repro 2 — `!nontemporal` dropped

Same shape, replace `!invariant.load !0` with `!nontemporal !{i32 1}`.
The two split scalar loads are emitted without `!nontemporal`. On x86
this means the backend cannot select streaming-load instructions
(`movntdqa` / `vmovntdqa`) for what the source programmer marked as
streaming.

## Repro 3 — `!range` dropped

Same shape with `!range !{i32 0, i32 16}`. The original load constrained
all four elements to `[0, 16)`. After scalarization the two emitted
`load i32` instructions have no range information, so later
KnownBits / ValueTracking-based folds (e.g. `icmp ult, 16` → `true`) no
longer fire on the loaded values.

## Severity table

| Metadata | Effect of loss |
| --- | --- |
| `!invariant.load` | LICM refuses to hoist; GVN refuses to widen-CSE; loads in loops cannot be sunk. The source programmer explicitly promised the memory is read-only — the optimizer no longer knows this. |
| `!nontemporal` | Streaming-store/load codegen lost; performance regression on bandwidth-bound kernels. |
| `!range` | KnownBits weakens; range-based folds in InstCombine/InstSimplify miss. May cascade through subsequent passes. |
| `!noundef` | Misses opportunities to fold `freeze` / `select-of-undef`. |
| `!dereferenceable`, `!dereferenceable_or_null` | Cascade of `isSafeToLoad` / `isDereferenceablePointer` checks pessimize. |

## Fix sketch

```cpp
auto *NewLoad = cast<LoadInst>(...);
NewLoad->setAlignment(ScalarOpAlignment);

// AAMetadata for constant indices (existing code).
if (auto *ConstIdx = dyn_cast<ConstantInt>(Idx)) {
  size_t Offset = ConstIdx->getZExtValue() * DL->getTypeStoreSize(ElemType);
  AAMDNodes OldAAMD = LI->getAAMetadata();
  NewLoad->setAAMetadata(OldAAMD.adjustForAccess(Offset, ElemType, *DL));
}

// NEW: load-level invariant/perf/aux metadata is element-agnostic
// (whole-load property) and is safe to propagate verbatim.
for (unsigned MDID : {LLVMContext::MD_invariant_load,
                      LLVMContext::MD_nontemporal,
                      LLVMContext::MD_noundef,
                      LLVMContext::MD_invariant_group,
                      LLVMContext::MD_annotation})
  if (MDNode *MD = LI->getMetadata(MDID))
    NewLoad->setMetadata(MDID, MD);

// !range is per-element-but-uniform-across-lanes for the source vector
// load, so the same range applies to each scalar load.
if (MDNode *Range = LI->getMetadata(LLVMContext::MD_range))
  NewLoad->setMetadata(LLVMContext::MD_range, Range);
```
