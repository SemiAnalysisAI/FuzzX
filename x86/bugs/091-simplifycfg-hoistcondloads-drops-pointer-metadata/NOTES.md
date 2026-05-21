# w42: hoistConditionalLoadsStores drops !nonnull / !align / !dereferenceable / !invariant.load

## File
`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` (HoistLoadsStoresWithCondFaulting path)

## Function
`hoistConditionalLoadsStores`, lines 1734-1846

## Description
When SimplifyCFG hoists a pair of conditionally-executed scalar loads / stores
through a masked.load / masked.store, the metadata transfer is asymmetric:

```cpp
if (const MDNode *Ranges = I->getMetadata(LLVMContext::MD_range))
  MaskedLoadStore->addRangeRetAttr(getConstantRangeFromMetadata(*Ranges));
I->dropUBImplyingAttrsAndUnknownMetadata({LLVMContext::MD_annotation});
...
MaskedLoadStore->copyMetadata(*I);
```

`dropUBImplyingAttrsAndUnknownMetadata` is called on `I` with only
`MD_annotation` in the keep list, then `copyMetadata` is called from the now
stripped `I`. The pre-comment justifies the loss with:

> // !nonnull, !align : Not support pointer type, no need to keep.

That comment is misleading: scalar pointer-typed loads ARE allowed through
this path (line 1769 only blocks vector loaded types). The masked.load
intrinsic supports pointer-typed lane elements.

Practical effect when the original is `load ptr, ptr %p, !nonnull
!dereferenceable !{i64 8}, !align !{i64 8}`:
- the resulting bitcast of the masked.load element is no longer known nonnull,
- subsequent uses can't reuse the dereferenceability fact when the mask is true.

Is it a bug?  Soundness-wise: NO. The masked.load result under a false mask is
the pass-through (often poison or zeroinitializer); neither would carry
nonnull/dereferenceable, so dropping is the conservative choice. The
`addRangeRetAttr` for `!range` is also correct only because the per-lane range
of a single-lane vector is the same constant range.

Status: missed-optimization / stale comment only. Not a miscompile.

Documented because the task description named this exact pattern; ruling it
out.
