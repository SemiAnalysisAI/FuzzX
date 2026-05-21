# w95: InstCombine `unpackLoadToAggregate` array path drops `!invariant.load` (and other non-AA metadata) on the synthesized element loads

## File

`llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp`, lines
788-832 inside `unpackLoadToAggregate` (the `ArrayType` branch).

## Code

```cpp
if (auto *AT = dyn_cast<ArrayType>(T)) {
  auto *ET = AT->getElementType();
  auto NumElements = AT->getNumElements();
  if (NumElements == 1) {
    LoadInst *NewLoad = IC.combineLoadToNewType(LI, ET, ".unpack");
    NewLoad->setAAMetadata(LI.getAAMetadata());
    return IC.replaceInstUsesWith(LI, IC.Builder.CreateInsertValue(
      PoisonValue::get(T), NewLoad, 0, Name));
  }
  ...
  for (uint64_t i = 0; i < NumElements; i++) {
    ...
    auto *L = IC.Builder.CreateAlignedLoad(AT->getElementType(), Ptr,
                                           EltAlign, Name + ".unpack");
    L->setAAMetadata(LI.getAAMetadata());     // <<< only AA metadata copied
    V = IC.Builder.CreateInsertValue(V, L, i);
    ...
  }
  ...
}
```

The corresponding **struct branch** on lines 768-782 also copies
`!invariant.load`:

```cpp
auto *L = IC.Builder.CreateAlignedLoad(...);
L->setAAMetadata(LI.getAAMetadata());
L->copyMetadata(LI, LLVMContext::MD_invariant_load);  // <<< present here
V = IC.Builder.CreateInsertValue(V, L, i);
```

The 1-element array fast path at line 788-794 also uses `combineLoadToNewType`
which goes through `copyMetadataForLoad` and *does* copy `!invariant.load`
(line 3141 of `Utils/Local.cpp`). So the multi-element array branch is the
only one in the function that silently drops invariant_load.

Additionally it drops every other non-AA metadata kind: `!nontemporal`,
`!noundef`, `!noalias_addrspace`, `!fpmath`, `!mem_parallel_loop_access`,
`!access_group`, etc.

## Concrete IR (reproduces against the local build)

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define [2 x i32] @arr_load_inv(ptr %p) {
  %v = load [2 x i32], ptr %p, align 4, !invariant.load !0, !tbaa !1
  ret [2 x i32] %v
}
!0 = !{}
!1 = !{!2, !2, i64 0}
!2 = !{!"int", !3, i64 0}
!3 = !{!"omnipotent char"}
```

`build/llvm-fuzzer/bin/opt -passes=instcombine -S`:

```llvm
define [2 x i32] @arr_load_inv(ptr %p) {
  %v.unpack = load i32, ptr %p, align 4, !tbaa !0          ; <-- !invariant.load gone
  %1 = insertvalue [2 x i32] poison, i32 %v.unpack, 0
  %v.elt1 = getelementptr inbounds nuw i8, ptr %p, i64 4
  %v.unpack2 = load i32, ptr %v.elt1, align 4, !tbaa !0    ; <-- !invariant.load gone
  %v3 = insertvalue [2 x i32] %1, i32 %v.unpack2, 1
  ret [2 x i32] %v3
}
```

`!tbaa` is preserved (because of `setAAMetadata`), but `!invariant.load` is
silently dropped on every element load. The same loss applies to
`!nontemporal`, `!noundef`, etc. Reproduced cleanly without dependencies on
other passes.

## Miscompile angle

Same class of issue as the InvariantGroup loss candidate in
`w95-instcombine-load-retype-drops-invariant-group.md` but in a different
spot: dropping `!invariant.load` blocks downstream `GVN` / `LICM` from
treating the per-element loads as immutable, which can change the call graph
when these loads feed into devirtualization or constant-propagation
post-instcombine.

It is also asymmetric: the *struct* branch preserves the metadata, but the
*array* branch silently drops it - so source-level `load [2 x i32]` and `load
{i32, i32}` produce different metadata profiles after InstCombine, even though
both should be equivalent from the language perspective.

The fix is a one-liner: add `L->copyMetadata(LI,
LLVMContext::MD_invariant_load);` (or better, use `copyMetadataForLoad`) on
line 825 to match the struct branch on line 780.

## Confidence

High that the metadata is dropped (verified by reproducer above).
Medium for an actual end-to-end miscompile - this is a missed optimization in
isolation, but reaches correctness territory when combined with passes that
require `!invariant.load` for safety of cross-block sinking (e.g.
`tryToSinkInstruction` on line 5609 of `InstructionCombining.cpp` consults
`MD_invariant_load`).
