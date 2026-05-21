# w252 — VectorCombine `scalarizeLoadExtract` drops !nontemporal and !invariant.load from new scalar loads

## Files / locations

- `llvm/lib/Transforms/Vectorize/VectorCombine.cpp:2116-2143`
  Function: `VectorCombine::scalarizeLoadExtract(LoadInst *LI, VectorType *VecTy, Value *Ptr)`

## Bug

`scalarizeLoadExtract` turns

```
%v = load <4 x i32>, ptr %p, align 16, !nontemporal !.., !invariant.load !..
%e0 = extractelement <4 x i32> %v, i32 0
%e1 = extractelement <4 x i32> %v, i32 1
...
```

into a separate scalar load per extracted element:

```cpp
auto *NewLoad = cast<LoadInst>(
    Builder.CreateLoad(ElemType, GEP, EI->getName() + ".scalar"));
...
if (auto *ConstIdx = dyn_cast<ConstantInt>(Idx)) {
  size_t Offset = ConstIdx->getZExtValue() * DL->getTypeStoreSize(ElemType);
  AAMDNodes OldAAMD = LI->getAAMetadata();
  NewLoad->setAAMetadata(OldAAMD.adjustForAccess(Offset, ElemType, *DL));
}
```

Only `AAMDNodes` (TBAA / alias.scope / noalias / TBAA-struct) is forwarded,
and only when the index is constant. **`!nontemporal`, `!invariant.load`,
`!access_group`, `!mmra`, `!align`, `!dereferenceable`, `!dereferenceable_or_null`
are silently dropped on every new scalar load.**

For dynamic (non-constant) indices the AAMetadata is dropped too.

## Reproducer

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(i32, i32)

define void @f(ptr align 16 dereferenceable(64) %p) {
  %v = load <4 x i32>, ptr %p, align 16, !nontemporal !0, !invariant.load !1
  %e0 = extractelement <4 x i32> %v, i32 0
  %e3 = extractelement <4 x i32> %v, i32 3
  call void @use(i32 %e0, i32 %e3)
  ret void
}

!0 = !{i32 1}
!1 = !{}
```

`opt -O2 -S` produces:

```llvm
define void @f(ptr ... align 16 ... dereferenceable(64) %p) ... {
  %e0 = load i32, ptr %p, align 16
  %1 = getelementptr inbounds nuw i8, ptr %p, i64 12
  %e3 = load i32, ptr %1, align 4
  tail call void @use(i32 %e0, i32 %e3)
  ret void
}
```

Both `!nontemporal` and `!invariant.load` are gone from BOTH new scalar
loads. `-passes='vector-combine' -S` shows the same drop (plus the gep
shape).

## Why this is wrong

- `!invariant.load` is correctness-relevant: it tells GVN/LICM the memory
  is never written. Dropping it makes other passes assume the location can
  change; a hoist that would have been legal under the original load is no
  longer recognized as legal.
- `!nontemporal` is a perf/ISA hint contract from the front-end (e.g.
  `__builtin_nontemporal_load`). It should propagate to derived loads.
- The pattern of using `AAMDNodes::adjustForAccess` is correct but
  incomplete — only AAMetadata is handled. The other kinds in the
  "supported for vectorization" list (see
  `VectorUtils.cpp:1053-1057` — `MD_nontemporal`, `MD_invariant_load`,
  `MD_access_group`, `MD_mmra`) need to be forwarded too.

## Fix sketch

After `NewLoad->setAAMetadata(...)` add:

```cpp
for (unsigned MDKind : {LLVMContext::MD_nontemporal,
                       LLVMContext::MD_invariant_load,
                       LLVMContext::MD_access_group,
                       LLVMContext::MD_mmra})
  if (MDNode *MD = LI->getMetadata(MDKind))
    NewLoad->setMetadata(MDKind, MD);
```

(For non-constant indices also forward the AAMetadata that doesn't depend
on offset, i.e. `Scope`, `NoAlias`.)
