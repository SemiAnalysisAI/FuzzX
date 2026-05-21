# DSE end-of-function elimination drops `!annotation` on dead store

**Component:** llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp

**Function:** `DSEState::eliminateDeadWritesAtEndOfFunction()` →
`deleteDeadInstruction()`

**Lines:** 2103-2141 (function body); deletion at 2134
(`deleteDeadInstruction(DefI);`).  The deletion helper
`deleteDeadInstruction(Instruction *, ...)` is at 2005-2065 and only calls
`salvageDebugInfo` / `salvageKnowledge` — it never inspects
`MD_annotation`.

## Pattern

A store to an alloca-like / byval / inalloca / `dead_on_return` location
is considered dead at the end of the function when no read clobber follows
it. DSE deletes it via `deleteDeadInstruction`. The store's `!annotation`
metadata (e.g. emitted by source-level instrumentation, sample-PGO, or
custom tooling that records "this store happened with reason X") is silently
discarded because the deletion path makes no attempt to forward it
anywhere.

## Bug

`deleteDeadInstruction` (2005-2065) only preserves debug info via
`salvageDebugInfo(*DeadInst)` (line 2017) and call-attribute knowledge via
`salvageKnowledge(DeadInst)` (line 2018). There is no transfer of
`MD_annotation`. When the store is the only carrier of that annotation,
its information is gone.

This is the same class of metadata-loss bug as the partial-store-merging
case (w395) but reached through the end-of-function elimination path.

## Confirmed via `opt -passes=dse` (x86_64, default `-O2`)

### Input IR
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use_byval(ptr byval(i32))

define void @f(ptr byval(i32) %p) {
  store i32 42, ptr %p, align 4, !annotation !0    ; dead at end of function
  ret void
}

!0 = !{!"byval_set"}
```

### After `opt -passes=dse -S`
```
define void @f(ptr byval(i32) %p) {
  ret void
}
```

The `!annotation` is gone. Also reproducible with a plain `alloca`:

```ll
define void @g() {
  %a = alloca i32
  store i32 7, ptr %a, !annotation !0
  ret void
}
```

Output after DSE: function body is just `ret void`, annotation lost.

Reachable via default x86 `-O2`.

## Fix sketch

`deleteDeadInstruction` needs an opportunity for callers to surface any
`!annotation` MD attached to the about-to-be-deleted instruction. Either
attach it to the function's `!annotation` summary, or call a hook similar
to `combineMetadata` against some surviving instruction in the same block.
At minimum, the loss should not be silent — debug info salvage is
preserved; `MD_annotation` deserves equivalent treatment.
