# w95: InstCombine load-of-select fold drops `!noundef`, `!invariant.load`, `!nontemporal`, and AA metadata on the two new loads

## File

`llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp`, lines
1140-1180 inside `InstCombinerImpl::visitLoadInst`.

## Code

```cpp
LoadInst *V1 = Builder.CreateLoad(LI.getType(), LoadOp1,
                                  LoadOp1->getName() + ".val");
LoadInst *V2 = Builder.CreateLoad(LI.getType(), LoadOp2,
                                  LoadOp2->getName() + ".val");
assert(LI.isUnordered() && "implied by above");
V1->setAlignment(Alignment);
V1->setAtomic(LI.getOrdering(), LI.getSyncScopeID());
V2->setAlignment(Alignment);
V2->setAtomic(LI.getOrdering(), LI.getSyncScopeID());
// It is safe to copy any metadata that does not trigger UB. Copy any
// poison-generating metadata.
V1->copyMetadata(LI, Metadata::PoisonGeneratingIDs);
V2->copyMetadata(LI, Metadata::PoisonGeneratingIDs);
return SelectInst::Create(SI->getCondition(), V1, V2, ...);
```

`Metadata::PoisonGeneratingIDs` (defined in
`llvm/include/llvm/IR/Metadata.h:146`) is the array
`{MD_range, MD_nonnull, MD_align, MD_nofpclass}`. Anything outside that set is
discarded on `V1` and `V2`.

## Bug

Metadata that survives transformation in *every other* load-cloning helper in
this file (see `copyMetadataForLoad` in `llvm/lib/Transforms/Utils/Local.cpp`)
is silently dropped here:

- `!noundef`           - guaranteed-defined load result.
- `!invariant.load`    - immutable memory location.
- `!nontemporal`       - cache-bypass hint.
- `!tbaa`, `!noalias`, `!alias.scope`, `!noalias.addrspace` - all AA metadata.
- `!dereferenceable`, `!dereferenceable_or_null`.
- `!noundef` and `!invariant.load` are not "poison-generating" per se, but they
  unlock downstream constant folding / load-CSE that the equivalent
  `load (select ...)` form would also unlock.

## Concrete IR (reproduces against the local build)

```llvm
@g1 = global i32 5, align 16
@g2 = global i32 7, align 16

define i32 @sel_drops_noundef(i1 %c) {
  %p = select i1 %c, ptr @g1, ptr @g2
  %v = load i32, ptr %p, align 4,
        !noundef !0, !nontemporal !1, !invariant.load !2
  %r = add i32 %v, %v
  ret i32 %r
}
!0 = !{}
!1 = !{i32 1}
!2 = !{}
```

`build/llvm-fuzzer/bin/opt -passes=instcombine -S`:

```llvm
define i32 @sel_drops_noundef(i1 %c) {
  %g1.val = load i32, ptr @g1, align 4   ; !noundef, !invariant.load, !nontemporal all gone
  %g2.val = load i32, ptr @g2, align 4   ; same: all metadata dropped
  %v = select i1 %c, i32 %g1.val, i32 %g2.val
  %r = shl i32 %v, 1
  ret i32 %r
}
```

## Miscompile angle

This is a *missed optimization* (dropping `!noundef` is conservative for
soundness), but it is observably wrong in pipeline interactions:

- `!invariant.load` on `@g1` / `@g2` would let later passes (GVN, LICM,
  ConstProp-with-loads) replace each new load with the constant initializer
  `5` / `7`. After this fold, those constant-prop opportunities are blocked
  because the new loads no longer carry the metadata.
- `!noundef` on the load lets `SimplifyDemandedBits` / `SROA` /
  `CorrelatedValuePropagation` widen the value's known bits and trigger
  `freeze` removal. Removing it pessimizes code that depends on
  freeze-removal heuristics.
- For `!tbaa`, `!alias.scope`, `!noalias`: these unlock alias-disambiguation
  on the new loads. Their loss prevents motion across stores even when the
  user provided the aliasing knowledge explicitly.

The fix is to use `copyMetadataForLoad(*V1, LI)` and `copyMetadataForLoad(*V2,
LI)` instead of `copyMetadata(LI, PoisonGeneratingIDs)`, mirroring the helper
that exists for exactly this purpose in `Utils/Local.cpp`.

## Confidence

High that the metadata is dropped (verified by reproducer above).
Medium that this manifests as a downstream miscompile rather than only as a
missed optimization. The closest known precedent is
`w63b-vectorcombine-scalarizeLoadExtract-strips-atomic.md` and
`w61-sroa-drops-atomic-on-promoted-loadstore.md` in this same candidates
folder.
