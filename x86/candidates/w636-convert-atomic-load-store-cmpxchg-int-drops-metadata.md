# w636: `convertAtomicLoadToIntegerType` / `convertAtomicStoreToIntegerType` / `convertCmpXchgToIntegerType` silently drop TBAA / alias.scope / noalias / pcsections metadata

## Severity
Miscompile risk (alias-analysis misinformation). Not currently caught by the
verifier, so it is silent.

## Source

`llvm/lib/CodeGen/AtomicExpandPass.cpp`

`convertAtomicLoadToIntegerType` (lines 556-576):

```cpp
LoadInst *AtomicExpandImpl::convertAtomicLoadToIntegerType(LoadInst *LI) {
  auto *M = LI->getModule();
  Type *NewTy = getCorrespondingIntegerType(LI->getType(), M->getDataLayout());

  ReplacementIRBuilder Builder(LI, *DL);
  Value *Addr = LI->getPointerOperand();

  auto *NewLI = Builder.CreateLoad(NewTy, Addr);
  NewLI->setAlignment(LI->getAlign());
  NewLI->setVolatile(LI->isVolatile());
  NewLI->setAtomic(LI->getOrdering(), LI->getSyncScopeID());
  // <-- NO call to copyMetadataForAtomic(*NewLI, *LI)
  ...
}
```

`convertAtomicStoreToIntegerType` (lines 697-712): same omission.

`convertCmpXchgToIntegerType` (lines 1418-1449): same omission.

Compare with the sister helper `convertAtomicXchgToIntegerType`
(lines 579-607) which correctly calls
`copyMetadataForAtomic(*NewRMWI, *RMWI)` at line 598 - and with
`expandAtomicRMWToCmpXchg` / `expandPartwordAtomicRMW` /
`widenPartwordAtomicRMW` which all pass a `MetadataSrc` instruction so the
copy actually happens.

`copyMetadataForAtomic` (lines 232-263) is the canonical filter for "metadata
that's safe to preserve when widening atomics" - it propagates `MD_tbaa`,
`MD_tbaa_struct`, `MD_alias_scope`, `MD_noalias`, `MD_noalias_addrspace`,
`MD_access_group`, `MD_mmra`, `MD_dbg`, plus AMDGPU-specific
`amdgpu.no.remote.memory` / `amdgpu.no.fine.grained.memory`.

`ReplacementIRBuilder` (lines 174-196) collects `MD_pcsections` for *every*
instruction it creates, so pcsections survives. But everything else in the
"safe to preserve" list is dropped for load/store/cmpxchg type conversion.

## Repro 1 - atomic FP load loses TBAA

```llvm
target triple = "x86_64-unknown-linux-gnu"

define float @load_f32_tbaa(ptr %p) {
  %v = load atomic float, ptr %p seq_cst, align 4, !tbaa !0
  ret float %v
}

!0 = !{!1, !1, i64 0}
!1 = !{!"float", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C++ TBAA"}
```

```console
$ llc -mtriple=x86_64-unknown-linux-gnu -stop-after=atomic-expand repro.ll -o -
...
define float @load_f32_tbaa(ptr %p) {
  %1 = load atomic i32, ptr %p seq_cst, align 4   ; <-- !tbaa !0 lost
  %2 = bitcast i32 %1 to float
  ret float %2
}
```

`shouldCastAtomicLoadInIR` returns `CastToInteger` for the FP load
(`X86ISelLowering.cpp:33000-33004`), `convertAtomicLoadToIntegerType` runs,
the integer load it creates carries no `!tbaa`.

## Repro 2 - atomic FP store loses TBAA

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @store_f32_tbaa(ptr %p, float %v) {
  store atomic float %v, ptr %p seq_cst, align 4, !tbaa !0
  ret void
}

!0 = !{!1, !1, i64 0}
!1 = !{!"float", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C++ TBAA"}
```

```console
$ llc -mtriple=x86_64-unknown-linux-gnu -stop-after=atomic-expand repro.ll -o -
...
define void @store_f32_tbaa(ptr %p, float %v) {
  %1 = bitcast float %v to i32
  store atomic i32 %1, ptr %p seq_cst, align 4   ; <-- !tbaa !0 lost
  ret void
}
```

## Repro 3 - atomic pointer cmpxchg loses TBAA

```llvm
target triple = "x86_64-unknown-linux-gnu"

define { ptr, i1 } @cas_ptr_tbaa(ptr %p, ptr %old, ptr %new) {
  %r = cmpxchg ptr %p, ptr %old, ptr %new seq_cst seq_cst, align 8, !tbaa !0
  ret { ptr, i1 } %r
}

!0 = !{!1, !1, i64 0}
!1 = !{!"ptr", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C++ TBAA"}
```

```console
$ llc -mtriple=x86_64-unknown-linux-gnu -stop-after=atomic-expand repro.ll -o -
...
define { ptr, i1 } @cas_ptr_tbaa(ptr %p, ptr %old, ptr %new) {
  %1 = ptrtoint ptr %old to i64
  %2 = ptrtoint ptr %new to i64
  %3 = cmpxchg ptr %p, i64 %1, i64 %2 seq_cst seq_cst, align 8   ; <-- !tbaa !0 lost
  ...
}
```

`processAtomicInstr` casts the cmpxchg to integer (`AtomicExpandPass.cpp:419-423`)
unconditionally for pointer compare operands, so the dropping path is on the
default x86-64 pipeline - no special attributes needed.

## Why this is a real correctness issue, not just QoI

`!tbaa`, `!alias.scope`, `!noalias`, and `!noalias_addrspace` participate in
the alias-analysis answer for subsequent passes. By dropping them, the atomic
load/store/cmpxchg becomes pessimistically "may alias" everything in the same
function. That's only one direction of the badness - if downstream IR retains
TBAA annotations on *other* loads/stores derived from the same source object,
the existing alias-analysis logic in MemorySSA / DSE / GVN can be confused
about whether the post-AtomicExpand integer access aliases the FP/pointer
accesses it was lowered from. In the wrong direction, this causes misoptimization
(e.g., DSE / GVN forwarding past an atomic that the optimizer wrongly believes
is `noalias`).

This is also visibly inconsistent with sibling code paths:

| Helper | Calls `copyMetadataForAtomic`? |
| ------ | ------------------------------ |
| `convertAtomicLoadToIntegerType` | **No** |
| `convertAtomicStoreToIntegerType` | **No** |
| `convertCmpXchgToIntegerType` | **No** |
| `convertAtomicXchgToIntegerType` | Yes (line 598) |
| `expandAtomicRMWToCmpXchg` (via `MetadataSrc`) | Yes (line 758) |
| `expandPartwordAtomicRMW` (via `MetadataSrc`) | Yes (line 758) |
| `widenPartwordAtomicRMW` | Yes (line 1145) |

## Suggested fix

Add `copyMetadataForAtomic(*NewLI, *LI);` (and the store / cmpxchg analogs)
after the new instruction's atomic/alignment/volatile bits are set in each of
the three helpers. They're all already in the same file with the static
helper in scope.

## opt/llc diff summary

- `opt`: not affected by default (atomic-expand isn't in the default pipeline).
- `llc -stop-after=atomic-expand`: the resulting IR has no `!tbaa` /
  `!alias.scope` / `!noalias` metadata on the rewritten load/store/cmpxchg, as
  shown above. Final assembly is correct in isolation but would have permitted
  invalid AA-driven optimizations had AtomicExpand run inside the middle-end
  pipeline (which is the situation when targets pre-run it).
