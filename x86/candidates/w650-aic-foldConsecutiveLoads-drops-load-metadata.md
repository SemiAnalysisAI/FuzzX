## AggressiveInstCombine::foldConsecutiveLoads drops all non-AA metadata on the merged wide load

`llvm/lib/Transforms/AggressiveInstCombine/AggressiveInstCombine.cpp:1438-1444`

```cpp
// Generate wider load.
NewLoad = Builder.CreateAlignedLoad(WiderType, Load1Ptr, LI1->getAlign(),
                                    LI1->isVolatile(), "");
NewLoad->takeName(LI1);
// Set the New Load AATags Metadata.
if (LOps.AATags)
  NewLoad->setAAMetadata(LOps.AATags);
```

When a chain of consecutive narrow loads of the form
`(zext(L1) << s1) | (zext(L2) << s2) | ...` is rewritten into a single wider
load (`foldConsecutiveLoads`), the new `LoadInst` is built fresh and only
the AA tags are explicitly copied from the merged loads (via `LOps.AATags`,
the concatenation of all the merged loads' AA metadata, set in `LOps`
update at line 1381-1391). Every other metadata kind that is attached to the
input loads is silently dropped — there is no call to `combineMetadataForCSE`
/ `combineMetadata` / `copyMetadata*` here. Concretely lost when present on
both narrow loads:

- `!nontemporal` — codegen hint that the loaded data is not reused; merged
  load should also be non-temporal if all inputs are.
- `!invariant.load` — semantic claim that the loaded memory never changes
  during program execution; trivially holds for the merged load too.
- `!noundef` — guarantee that the loaded value is not undef/poison;
  merged value is `or` of zexts of guaranteed-defined narrow values, so
  it is also guaranteed defined.
- `!invariant.group` — useful for devirtualization preservation.
- DIAssignID (used for `dbg.assign` linkage), DebugLoc merging.

This is a transform-quality bug (not an immediate miscompile): downstream
passes that rely on these hints (LICM hoisting via `!invariant.load`,
codegen non-temporal stream selection, `DSE`/`MemorySSA` queries via
`!invariant.group`, etc.) are deprived of information that the input
clearly justified. The fix is the standard `combineMetadataForCSE(NewLoad,
LI1)` (or a manual `combineMetadata` over all merged loads, since `LI1` is
only one of the chain — `LOps` already keeps track of the others through
the rolling AATags concat).

### Repro (x86-64, `-passes=aggressive-instcombine`)

```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i64 @merge_loads_drop_meta(ptr %p) {
  %p1 = getelementptr inbounds i8, ptr %p, i64 4
  %l1 = load i32, ptr %p,  align 8, !nontemporal !0, !invariant.load !1, !noundef !1
  %l2 = load i32, ptr %p1, align 4, !nontemporal !0, !invariant.load !1, !noundef !1
  %z1 = zext i32 %l1 to i64
  %z2 = zext i32 %l2 to i64
  %sh = shl  i64 %z2, 32
  %r  = or   i64 %sh, %z1
  ret i64 %r
}

!0 = !{i32 1}
!1 = !{}
```

### Diff `opt -passes=aggressive-instcombine -S` (LLVM 23.0.0git):

Input has each narrow load annotated with `!nontemporal`, `!invariant.load`,
`!noundef`. After AIC:

```ll
define i64 @merge_loads_drop_meta(ptr %p) {
  %l1 = load i64, ptr %p, align 8
  ret i64 %l1
}
```

All metadata gone from the merged wide load. The same drop happens for a
chain of 4 i16 loads merged into one i64 load (same pattern).

### Where the merge does happen

The path is `foldUnusualPatterns -> foldConsecutiveLoads -> foldLoadsRecursive`
(AggressiveInstCombine.cpp:2444, 1402, 1253). The AA-metadata plumbing is
done in `foldLoadsRecursive` (lines 1381-1391) but only AA metadata is
threaded through `LOps`. The new load is created at 1438-1440 with only
AA-tags re-applied. No other metadata source is consulted.

### Suggested fix

In `foldConsecutiveLoads` after building `NewLoad`, walk all the loads that
were merged (the chain can be reconstructed by tracking each `LI1`/`LI2`
during recursion, or by collecting them into a `SmallVector<LoadInst*>` in
`LOps`) and call:

```cpp
combineMetadataForCSE(NewLoad, LI1, /*DoesKMove=*/true);
for (LoadInst *L : OtherMergedLoads)
  combineMetadata(NewLoad, L,
                  /*KnownIDs=*/{LLVMContext::MD_nontemporal,
                                LLVMContext::MD_invariant_load,
                                LLVMContext::MD_invariant_group,
                                LLVMContext::MD_noundef,
                                LLVMContext::MD_alias_scope,
                                LLVMContext::MD_noalias,
                                LLVMContext::MD_tbaa},
                  /*AAOnly=*/false);
```

(or, equivalently, use `Inst::andIRFlags` plus the standard metadata-merging
helpers in `llvm/Transforms/Utils/Local.h`.)

### Notes on the other two hunting hypotheses

- "TryToShrink intrinsic loses fmf" — TruncInstCombine (TruncInstCombine.cpp:48-85)
  is integer-only by construction (no FP opcodes in `getRelevantOperands`),
  and `foldSqrt` (AggressiveInstCombine.cpp:879) does propagate FMF by
  passing the original `Call` to `Builder.CreateIntrinsic(Intrinsic::sqrt,
  Ty, Arg, Call, "sqrt")`. Verified by hand-running `reassoc nnan nsz`
  sqrt: flags preserved on `@llvm.sqrt.f64`. The other intrinsic-creating
  helpers (`foldSelectSplitCTTZ/CTLZ`, `tryToRecognizeTableBasedCttz/Log2`,
  `tryToFPToSat`, `foldGuardedFunnelShift`, `replaceWithPopCount`,
  `foldMulHigh`) operate on integer or saturating-FP intrinsics that don't
  participate in FMF.
- "foldLoadsRecursive vector merge wrong align/MMO" — `foldConsecutiveLoads`
  bails at line 1406-1407 with `if (isa<VectorType>(I.getType())) return false;`
  so the vector path is unreachable. The scalar alignment is `LI1->getAlign()`
  (line 1439) where `LI1` is, after the swap at 1351, always the
  lowest-offset load in the chain — that is the correct alignment for the
  new merged load that starts at that address.
