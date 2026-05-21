# DSE `storeIsNoop` (store of init value to zeroed allocator) drops `!annotation`

**Component:** llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp

**Function:** `DSEState::storeIsNoop()` → deletion in
`DSEState::eliminateDeadDefs(MemoryDefWrapper&)`.

**Lines:** 2371-2379 (`storeIsNoop` constant-init path):
```
Constant *InitC =
    getInitialValueOfAllocation(DefUO, &TLI, StoredConstant->getType());
if (InitC && InitC == StoredConstant)
  return MSSA.isLiveOnEntryDef(
    MSSA.getSkipSelfWalker()->getClobberingMemoryAccess(Def, BatchAA));
```
Deletion at line 2747
(`deleteDeadInstruction(KillingLocWrapper.DefInst);`).
`getInitialValueOfAllocation` (Analysis/MemoryBuiltins.cpp:418-439) returns
`Constant::getNullValue(Ty)` for any allocator with
`AllocFnKind::Zeroed` (e.g. `calloc`, libc-style or `allockind("alloc,zeroed")`).

## Pattern

A store of `0` to a freshly-`calloc`'d region is recognized as a no-op
because the allocator already zeroed the memory. DSE deletes the store via
`deleteDeadInstruction` (line 2747) which, like every other DSE deletion
site, ignores `MD_annotation`.

## Bug

Same root-cause family as w395 / w396 / w397 (DSE never propagates
`!annotation` when deleting a store) but reached via the
`storeIsNoop` → `getInitialValueOfAllocation` arm. This path fires for
real-world code that explicitly zeros a `calloc`'d struct for clarity /
defensiveness — the deletion is sound but the annotation is silently lost.

## Confirmed via `opt -passes=dse` (x86_64, default `-O2`)

### Input IR
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare noalias ptr @calloc(i64 noundef, i64 noundef)
  nounwind allockind("alloc,zeroed") allocsize(0,1)
  memory(inaccessiblemem: readwrite, errnomem: write) "alloc-family"="malloc"

declare void @use(i32)

define void @t() {
  %p = call ptr @calloc(i64 1, i64 4)
  store i32 0, ptr %p, align 4, !annotation !0    ; recognized as no-op by storeIsNoop
  %v = load i32, ptr %p, align 4
  call void @use(i32 %v)
  ret void
}

!0 = !{!"calloc_zero_init"}
```

### After `opt -passes=dse -S`
```
define void @t() {
  %p = call ptr @calloc(i64 1, i64 4)
  %v = load i32, ptr %p, align 4
  call void @use(i32 %v)
  ret void
}
```

`!annotation !0` is gone. Default x86 `-O2` also folds the load to `0` and
removes the `calloc` entirely — the annotation is irretrievably gone before
any later pass can transfer it elsewhere.

## Notes

Together with w395 (partial merge), w396 (end-of-function), and w397
(lifetime.end terminator), this brings the count to **four** independent
DSE deletion sites that silently drop `!annotation`. The unified fix is to
make `deleteDeadInstruction` either preserve `MD_annotation` onto a
surviving sibling or surface a callback so the caller can.
