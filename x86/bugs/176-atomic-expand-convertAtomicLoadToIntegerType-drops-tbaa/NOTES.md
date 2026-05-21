# w108: AtomicExpandPass convertAtomicLoadToIntegerType / convertAtomicStoreToIntegerType drop !tbaa, !noalias

## Root cause
Two sibling helpers in `llvm/lib/CodeGen/AtomicExpandPass.cpp` cast a
float/vector atomic load or store to an equivalent integer type so the
backend can pattern-match it. Both correctly propagate volatile, ordering,
and syncscope; neither calls `copyMetadataForAtomic`.

`convertAtomicLoadToIntegerType` (line 556-575):
```
auto *NewLI = Builder.CreateLoad(NewTy, Addr);
NewLI->setAlignment(LI->getAlign());
NewLI->setVolatile(LI->isVolatile());
NewLI->setAtomic(LI->getOrdering(), LI->getSyncScopeID());
// BUG: no copyMetadataForAtomic(*NewLI, *LI);
```

`convertAtomicStoreToIntegerType` (line 697-712):
```
StoreInst *NewSI = Builder.CreateStore(NewVal, Addr);
NewSI->setAlignment(SI->getAlign());
NewSI->setVolatile(SI->isVolatile());
NewSI->setAtomic(SI->getOrdering(), SI->getSyncScopeID());
// BUG: no copyMetadataForAtomic(*NewSI, *SI);
```

The sibling `convertAtomicXchgToIntegerType` at line 578-607 DOES call
`copyMetadataForAtomic(*NewRMWI, *RMWI)` at line 598 -- so we have direct
evidence from the same file that this metadata copy is the expected pattern.

## Trigger condition (x86, direct)
`X86TargetLowering::shouldCastAtomicLoadInIR` returns `CastToInteger` for
any floating-point or vector-of-floating-point atomic load
(X86ISelLowering.cpp:33000-33004). The sibling `shouldCastAtomicStoreInIR`
behaves analogously.

Reproducer:
```
target triple = "x86_64-unknown-linux-gnu"

define float @test_castload_tbaa(ptr %p) {
  %r = load atomic float, ptr %p seq_cst, align 4, !tbaa !2, !noalias !6
  ret float %r
}

define float @test_castload_volatile(ptr %p) {
  %r = load atomic volatile float, ptr %p seq_cst, align 4, !tbaa !2, !noalias !6
  ret float %r
}

define void @test_caststore_tbaa(ptr %p, float %v) {
  store atomic float %v, ptr %p seq_cst, align 4, !tbaa !2, !noalias !6
  ret void
}

!0 = !{!"alias-domain"}
!1 = !{!"alias-scope-a", !0}
!2 = !{!3, !3, i64 0}
!3 = !{!"int", !4, i64 0}
!4 = !{!"omnipotent char", !5, i64 0}
!5 = !{!"Simple C/C++ TBAA"}
!6 = !{!1}
```

After `llc -mtriple=x86_64-unknown-linux-gnu -stop-after=atomic-expand`:
```
define float @test_castload_tbaa(ptr %p) {
  %1 = load atomic i32, ptr %p seq_cst, align 4          ; <-- no !tbaa, no !noalias
  %2 = bitcast i32 %1 to float
  ret float %2
}

define float @test_castload_volatile(ptr %p) {
  %1 = load atomic volatile i32, ptr %p seq_cst, align 4 ; <-- no !tbaa, no !noalias
  %2 = bitcast i32 %1 to float
  ret float %2
}

define void @test_caststore_tbaa(ptr %p, float %v) {
  %1 = bitcast float %v to i32
  store atomic i32 %1, ptr %p seq_cst, align 4           ; <-- no !tbaa, no !noalias
  ret void
}
```

Original IR had `!tbaa !2` and `!noalias !6`; expanded IR has neither.

## Why this is a miscompile (vs. a quality issue)
The expanded IR is then run through the rest of the codegen pipeline AND
re-exposed to IR-level analyses (e.g. via `-passes='atomic-expand,gvn'`
under the new pass manager, or LTO that re-invokes IR passes after target
lowering hooks). Concretely:

1. The original `load atomic float, ptr %p ... !tbaa !2` carried a TBAA tag
   that ensured a TBAA-disjoint store of a different type at the same address
   could be moved past it. After dropping the tag, GVN/MemCpyOpt cannot prove
   the same disjointness, but conversely a *different* sibling load with
   `!tbaa !2` may now be CSE-merged with the untagged load because the
   untagged load is treated as "compatible with any TBAA tag" by
   `MDNode::getMostGenericTBAA`. The merged load result is then used for
   both, but it is only marked atomic with one set of metadata -- producing
   inconsistent aliasing decisions for the same SSA value.

2. The dropped `!noalias` permits a later AA-driven sink to move a load
   tagged `!alias.scope !6` past this atomic load. The original IR with
   `!noalias !6` on this atomic load promised that no `!alias.scope !6`
   access could be moved across it. The expanded IR makes no such promise.

3. The volatile reproducer (`test_castload_volatile`) preserves volatile
   (good) but still loses TBAA/noalias (bad). Volatile prevents elision,
   but does NOT prevent reordering of *non-volatile* accesses across the
   volatile load; only the metadata tags do.

## Fix
Add `copyMetadataForAtomic(*NewLI, *LI);` after line 567 and
`copyMetadataForAtomic(*NewSI, *SI);` after line 709. Match the sibling
`convertAtomicXchgToIntegerType` at line 598.

## Also affected: expandAtomicLoadToCmpXchg
The same TBAA/noalias drop happens in `expandAtomicLoadToCmpXchg` at line
678. With `cx16` and i128 atomic load:
```
define i128 @test_load_cmpxchg_md(ptr %p) {
  %r = load atomic i128, ptr %p seq_cst, align 16, !tbaa !2, !noalias !6, !pcsections !7
  ret i128 %r
}
```
becomes (after `llc -mattr=+cx16 -stop-after=atomic-expand`):
```
%1 = cmpxchg ptr %p, i128 0, i128 0 seq_cst seq_cst, align 16, !pcsections !0
%loaded = extractvalue { i128, i1 } %1, 0, !pcsections !0
```
The pcsections survives only because `ReplacementIRBuilder` collects it;
`!tbaa` and `!noalias` are silently dropped.

## Related bugs
- Sibling of `convertAtomicXchgToIntegerType` (line 598) which gets it right.
- #066: same call site, identified `volatile`/`syncscope` drop in
  expandAtomicLoadToCmpXchg. This entry covers the TBAA/noalias drop in two
  additional callers in the same file.
- #088 (`w88-convertcmpxchgtoint-drops-md.md`): the sibling
  `convertCmpXChgToIntegerType` also has a similar pattern.
