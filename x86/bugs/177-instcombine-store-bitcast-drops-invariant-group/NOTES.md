# w106: InstCombine store-of-bitcast fold drops `!invariant.group`

**File:** `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp`
**Function:** `combineStoreToNewValue` (~line 616) invoked from `combineStoreToValueType` (~line 1296).

## Root cause

When InstCombine folds `store (bitcast X to T2), %p` into `store X, %p`, it routes
through `combineStoreToNewValue`. That function copies `SI.getAllMetadata()` into a
switch and silently drops every kind not listed. The switch lists `MD_dbg`,
`MD_DIAssignID`, `MD_tbaa`, `MD_prof`, `MD_fpmath`, `MD_tbaa_struct`,
`MD_alias_scope`, `MD_noalias`, `MD_nontemporal`, `MD_mem_parallel_loop_access`,
`MD_access_group` — but **not** `MD_invariant_group`. Since the bitcast is purely
type-change (no address change), `!invariant.group` MUST be preserved.

This is structurally identical to known bug #163 (load-retype drops invariant.group
in `copyMetadataForLoad`) but on the store side.

## Reproducer

```llvm
; opt -passes=instcombine -S
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"

define void @f(ptr %p, i64 %x) {
  %fp = bitcast i64 %x to double
  store double %fp, ptr %p, align 8, !invariant.group !0
  ret void
}
!0 = !{}
```

### Before
```
store double %fp, ptr %p, align 8, !invariant.group !0
```

### After (opt diff)
```
store i64 %x, ptr %p, align 8           ; !invariant.group GONE
```

## Why this is a miscompile

`!invariant.group` lets the optimizer reason about devirtualization safety
(C++ vptr / strict-aliasing class). A later GVN/loadCSE that sees a peer
`load !invariant.group` from the same group address would normally be forbidden
from forwarding through *any* store that does not carry the same group token —
losing the marker on the store can cause a peer load to be forwarded across a
true vptr-changing store (placement-new), substituting the wrong vtable.

## Fix

Add `case LLVMContext::MD_invariant_group:` to the "directly apply" arm of the
switch in `combineStoreToNewValue` (alongside `MD_nontemporal` etc.).
