# DSE partial-overlap store merging drops !nontemporal metadata on killing store

**Component:** llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp

**Function:** `DSEState::eliminateDeadDefs(MemoryLocationWrapper&)` partial-merge
branch + `tryToMergePartialOverlappingStores` helper.

**Lines:** 2683-2706 (call site), 770-814 (helper).

**Pattern:** DSE merges two partially-overlapping integer stores. The "killing"
store carries `!nontemporal` metadata; the "dead" store does not. The merge
silently deletes the killing store and updates only the dead store's stored
*value*. The `!nontemporal` hint is lost, so on x86 the resulting code uses a
regular cached `mov` instead of `movntiq` (MOVNTI), changing the cache /
memory-ordering behavior.

## Bug

`tryToMergePartialOverlappingStores` (770-814) requires that both `KillingI` and
`DeadI` are constant-int stores of typeSizeEqualsStoreSize, and that no
intervening writes happen. It then returns a merged constant. The caller (line
2683-2706) does:
```
DeadSI->setOperand(0, Merged);    // only the value changes; metadata kept as-is on DeadSI
deleteDeadInstruction(KillingSI, &Deleted);
```

KillingSI is erased outright. Any metadata on KillingSI (including
`!nontemporal`, `!noalias`, `!alias.scope`, `!invariant.group`, etc.) is
dropped. The merged store inherits *only* DeadSI's metadata.

On x86, `!nontemporal` on a wide integer store causes the backend to emit
`movntiq` / `movntdq` (cache-bypassing stores with weaker ordering). After DSE
folds the killing store into the dead store, the merged store is a regular
cached `movq`. Programs that rely on NT semantics (cache-flushing patterns,
streaming writes to write-combining memory, MMIO via WC-region) observe
different memory behavior.

## Confirmed via opt + llc

### IR before DSE
```ll
target triple = "x86_64-unknown-linux-gnu"
define void @t(ptr %p) {
  store i128 0, ptr %p, align 16
  %p2 = getelementptr i8, ptr %p, i64 4
  store i64 -1, ptr %p2, align 4, !nontemporal !0
  ret void
}
!0 = !{i32 1}
```

After `opt -passes=dse`:
```ll
define void @t(ptr %p) {
  store i128 79228162514264337589248983040, ptr %p, align 16
  ret void
}
```
The `!nontemporal` is gone.

### llc output (x86_64, -mattr=+sse2)

**Without DSE (original IR):**
```
xorps   %xmm0, %xmm0
movaps  %xmm0, (%rdi)
movq    $-1, %rax
movntiq %rax, 4(%rdi)      ; <-- non-temporal store
retq
```

**After DSE:**
```
movl    $4294967295, %eax
movq    %rax, 8(%rdi)
movabsq $-4294967296, %rax
movq    %rax, (%rdi)         ; <-- regular cached stores
retq
```

The `movntiq` (NON-TEMPORAL store) is replaced by a regular cached `movq`. NT
stores are weakly-ordered with respect to other accesses and bypass the cache;
this difference is directly observable.

## Fix sketch

Before merging, require KillingSI to have no metadata that DSE doesn't know how
to combine - or at minimum bail when KillingSI has `!nontemporal`, ordered
atomic semantics (already covered partly), or other side-effecting metadata.

Concretely in `tryToMergePartialOverlappingStores`, add:
```
if (KillingI->hasMetadata(LLVMContext::MD_nontemporal) &&
    !DeadI->hasMetadata(LLVMContext::MD_nontemporal))
  return nullptr;
```
(and ideally check the union of relevant MD kinds, transferring the union to
DeadI on success, similar to how MemoryDependenceAnalysis-era passes handle
metadata-preservation rules.)
