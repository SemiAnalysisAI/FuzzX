# w106: InstCombine store-of-bitcast fold drops `!noalias.addrspace`

**File:** `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp`
**Function:** `combineStoreToNewValue` (~line 616).

## Root cause

The metadata-copy switch in `combineStoreToNewValue` enumerates kinds that
"directly apply" to a retyped store (tbaa, alias.scope, noalias, nontemporal,
mem_parallel_loop_access, access_group, ...). It does **not** list
`LLVMContext::MD_noalias_addrspace`, so the metadata is silently dropped any
time `store (bitcast X to T2)` is folded.

`copyMetadataForLoad` (Local.cpp:3119) correctly *does* propagate
`MD_noalias_addrspace`. The asymmetry between the load and store sides is the
smoking gun.

## Reproducer

```llvm
; opt -passes=instcombine -S
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"

define void @f(ptr %p, i64 %x) {
  %fp = bitcast i64 %x to double
  store double %fp, ptr %p, align 8, !noalias.addrspace !0
  ret void
}
!0 = !{i32 5, i32 6}
```

### Before
```
store double %fp, ptr %p, align 8, !noalias.addrspace !0
```

### After (opt diff)
```
store i64 %x, ptr %p, align 8           ; !noalias.addrspace GONE
```

A combined trace verifies the asymmetry: with `!nontemporal !{i32 1}` plus
`!invariant.group !{}` plus `!noalias.addrspace !{i32 5, i32 6}` on the store,
only `!nontemporal` survives the bitcast fold — both `!invariant.group` and
`!noalias.addrspace` vanish.

## Why this is a miscompile

`!noalias.addrspace` constrains the set of address spaces with which the access
can alias. Dropping it lets a subsequent alias-analysis-driven pass conclude
that the store *may* alias addresses in the excluded address spaces, but more
importantly the store is now a *weaker* witness for AA queries from other
instructions that legitimately rely on the noalias.addrspace promise — a peer
load in an excluded address space could be incorrectly forwarded or removed
because the store no longer announces the noalias constraint that would have
killed that fold.

## Fix

Add `case LLVMContext::MD_noalias_addrspace:` to the "directly apply" arm of
the switch in `combineStoreToNewValue`, matching what `copyMetadataForLoad`
already does for loads.
