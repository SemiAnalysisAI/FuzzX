# 221 — InstCombine `mergeStoreIntoSuccessor` drops `!nontemporal` (and `!invariant.group`/`!access_group`) on the merged store

Component: `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp` lines ~1631-1745 (`InstCombinerImpl::mergeStoreIntoSuccessor`)

When two stores in successor blocks are merged into a single store of a `phi` value, the new store is constructed with no metadata; only `dbg`, `DIAssignID`, and `AAMetadata` are transferred. `MD_nontemporal`, `MD_invariant_group`, `MD_access_group`, `MD_mem_parallel_loop_access`, `MD_noalias_addrspace` are silently dropped — even when BOTH source stores carried identical hints.

## Reproducer

Both branches' stores have `!nontemporal !0`. After `opt -passes=instcombine -S`:
```
%storemerge = phi i32 [ %y, %f ], [ %x, %t ]
store i32 %storemerge, ptr %p, align 4   ; <-- !nontemporal LOST
```

## Severity

Default x86 -O2. NT hint silently dropped on merged successor stores. Same backend effect as #140/#192: cached MOV instead of MOVNT.

## Fix

After constructing the merged store, call `combineMetadataForCSE` on the two source stores, mirroring the SimplifyCFG `mergeConditionalStores` fix shape.
