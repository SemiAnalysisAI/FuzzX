# w251 — VectorCombine `vectorizeLoadInsert` drops all metadata when widening scalar load to vector load

## Files / locations

- `llvm/lib/Transforms/Vectorize/VectorCombine.cpp:343-352`
  Function: `VectorCombine::vectorizeLoadInsert(Instruction &I)`

## Bug

`vectorizeLoadInsert` matches `insertelement poison, load <scalar> %p, 0`
and rewrites the scalar load + insert into a wider vector load + shuffle:

```
  IRBuilder<> Builder(Load);
  Value *CastedPtr =
      Builder.CreatePointerBitCastOrAddrSpaceCast(SrcPtr, Builder.getPtrTy(AS));
  Value *VecLd = Builder.CreateAlignedLoad(MinVecTy, CastedPtr, Alignment);
  VecLd = Builder.CreateShuffleVector(VecLd, Mask);

  replaceValue(I, *VecLd);
```

The newly created `Builder.CreateAlignedLoad(MinVecTy, ...)` at line 347 has
no `copyMetadata`, no `setAAMetadata`. All the scalar load's TBAA / scope /
noalias / nontemporal / invariant.load / range metadata is dropped on the
widened vector load.

## Reproducer

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define <4 x i32> @f(ptr align 16 dereferenceable(64) %p) {
  %v = load i32, ptr %p, align 16, !nontemporal !0, !tbaa !1, !range !5
  %ins = insertelement <4 x i32> poison, i32 %v, i32 0
  ret <4 x i32> %ins
}

!0 = !{i32 1}
!1 = !{!2, !2, i64 0}
!2 = !{!"int", !3, i64 0}
!3 = !{!"omnipotent char", !4, i64 0}
!4 = !{!"Simple C/C++ TBAA"}
!5 = !{i32 0, i32 100}
```

`opt -passes='vector-combine' -S` produces:

```llvm
define <4 x i32> @f(ptr align 16 dereferenceable(64) %p) {
  %1 = load <4 x i32>, ptr %p, align 16
  %ins = shufflevector <4 x i32> %1, <4 x i32> poison, <4 x i32>
                       <i32 0, i32 poison, i32 poison, i32 poison>
  ret <4 x i32> %ins
}
```

`!nontemporal`, `!tbaa`, `!range` all dropped. `-O2 -S` produces the same
load with no metadata.

## Why this is wrong

- Same logic as w250: at minimum the TBAA / alias-scope / noalias is safe to
  transfer (the load is the same starting address, same or larger size; we
  proved `isSafeToLoadUnconditionally` above). Dropping them loses alias
  facts.
- `!range` is type-invalid to forward as-is because it was for `i32` and
  the new load is `<4 x i32>`. But the right thing is to convert it via
  `AAMDNodes::adjustForAccess` /  drop only the truly-invalid kinds, not
  drop everything.
- Compare the analogous transform `shrinkLoadForShuffles`
  (`VectorCombine.cpp:5650-5652`) which does the safe thing
  (`NewLoad->copyMetadata(I)`).

## Fix sketch

After line 347, copy the safe metadata kinds, e.g.
```cpp
AAMDNodes AA = Load->getAAMetadata();
cast<LoadInst>(VecLd)->setAAMetadata(AA);
if (MDNode *MD = Load->getMetadata(LLVMContext::MD_nontemporal))
  cast<LoadInst>(VecLd)->setMetadata(LLVMContext::MD_nontemporal, MD);
if (MDNode *MD = Load->getMetadata(LLVMContext::MD_invariant_load))
  cast<LoadInst>(VecLd)->setMetadata(LLVMContext::MD_invariant_load, MD);
```
(Don't blindly copy `!range`/`!noundef`/`!align` — those reference the old
scalar type.)
