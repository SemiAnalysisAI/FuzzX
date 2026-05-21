file: llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp:1149-1179

In `InstCombinerImpl::visitLoadInst`, the transform
`load(select(cond, p1, p2))` -> `select(cond, load p1, load p2)` only
copies poison-generating metadata to the two new loads:

```cpp
V1->copyMetadata(LI, Metadata::PoisonGeneratingIDs);
V2->copyMetadata(LI, Metadata::PoisonGeneratingIDs);
```

`Metadata::PoisonGeneratingIDs` (llvm/IR/Metadata.h:146) only covers
MD_range, MD_nonnull, MD_align, MD_nofpclass.

All other LOAD-relevant metadata is silently dropped: !tbaa,
!alias.scope, !noalias, !nontemporal, !invariant.load,
!mem_parallel_loop_access, !access_group, !noundef, !tbaa_struct,
!fpmath, !noalias_addrspace, !dereferenceable, !dereferenceable_or_null.

Contrast with copyMetadataForLoad (Local.cpp:3119-3177), which handles
ALL of those for type-changing load rewrites. The select-of-load splitter
should use copyMetadataForLoad (or a renamed variant that handles
"clone-for-same-type-different-pointer") for both V1 and V2.

opt diff (instcombine):

  ; Input
  define i32 @f(i1 %c, ptr %a, ptr %b) {
    %p = select i1 %c, ptr %a, ptr %b
    %v = load i32, ptr %p, align 4, !nontemporal !0, !tbaa !1, !alias.scope !4, !noundef !6
    ret i32 %v
  }
  !0 = !{i32 1}
  !1 = !{!2, !2, i64 0}
  !2 = !{!"x", !3}
  !3 = !{!"omnipotent char"}
  !4 = !{!5}
  !5 = distinct !{!5, !"scope"}
  !6 = !{}

  ; Output (after instcombine)
  define i32 @f(i1 %c, ptr %a, ptr %b) {
    %a.val = load i32, ptr %a, align 4              ; <-- lost tbaa/scope/nontemporal/noundef
    %b.val = load i32, ptr %b, align 4              ; <-- lost tbaa/scope/nontemporal/noundef
    %v = select i1 %c, i32 %a.val, i32 %b.val
    ret i32 %v
  }

Impact: classified as missed-opt under current LangRef (these are hints
or only enable optimizations). However:

- !invariant.load drop forces re-loads later that could have been CSE'd
- !tbaa/!alias.scope drop disables NoAlias-driven DSE and load CSE
- !nontemporal drop forfeits MOVNT codegen choice
- !noundef drop blocks downstream "value is well-defined" reasoning
- !mem_parallel_loop_access / !access_group drop blocks loop vec/par

The latent wrong-code risk lies in inconsistency: every other clone
site in this file uses copyMetadataForLoad. A future maintainer
adding poison-generating semantics to a metadata kind that's currently
treated as a hint would create a real miscompile here, because the
transform silently drops it instead of recognizing it.

Fix: replace the two `copyMetadata(LI, PoisonGeneratingIDs)` calls with
`copyMetadataForLoad(*V1, LI)` / `copyMetadataForLoad(*V2, LI)`, after
auditing that each metadata kind there is correct to duplicate (the
header comment on copyMetadataForLoad and the existing usages indicate
yes for all listed kinds).
