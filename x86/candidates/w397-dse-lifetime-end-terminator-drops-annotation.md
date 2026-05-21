# DSE `llvm.lifetime.end` memory-terminator deletion drops `!annotation`

**Component:** llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp

**Function:** `DSEState::eliminateDeadDefs(MemoryLocationWrapper&)`
mem-terminator branch.

**Lines:** 2660-2668 (the `isMemTerminatorInst(KillingLocWrapper.DefInst)`
arm of `eliminateDeadDefs`). Deletion at 2666
(`deleteDeadInstruction(DeadLocWrapper.DefInst, &Deleted);`).  Terminator
classification at 1548-1552 (`isMemTerminatorInst` accepts
`Intrinsic::lifetime_end`). Memory location at 1535-1546 (`getLocForTerminator`).

## Pattern

A `store` to an alloca is "killed" by a subsequent `llvm.lifetime.end` on
the same alloca because lifetime.end terminates all reads of the lifetime.
DSE detects this via `isMemTerminator`, then deletes the dead store via
`deleteDeadInstruction(DeadLocWrapper.DefInst, &Deleted)` at line 2666 with
no consideration for non-debug metadata.

Any `!annotation` MD attached to the dead store is silently dropped because
`deleteDeadInstruction` (2005-2065) only salvages debug info / knowledge,
not `!annotation` (see also w396).

## Bug

The terminator-kill branch (lines 2660-2668) is structurally identical to
the at-end-of-function deletion in that respect: the dead store goes away
and its annotation goes with it. No metadata is forwarded onto the
`lifetime.end` call (and even if it were, `lifetime.end` is itself often
deleted/ignored by later passes).

## Confirmed via `opt -passes=dse` (x86_64, default `-O2`)

### Input IR
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.lifetime.start(i64, ptr)
declare void @llvm.lifetime.end(i64, ptr)

define void @f() {
  %a = alloca i32, align 4
  call void @llvm.lifetime.start(i64 4, ptr %a)
  store i32 42, ptr %a, align 4, !annotation !0          ; dead (killed by lifetime.end)
  call void @llvm.lifetime.end(i64 4, ptr %a)
  ret void
}

!0 = !{!"asserted_value"}
```

### After `opt -passes=dse -S`
```
define void @f() {
  %a = alloca i32, align 4
  call void @llvm.lifetime.start.p0(ptr %a)
  call void @llvm.lifetime.end.p0(ptr %a)
  ret void
}
```

The `!annotation` is gone. Reachable via default x86 `-O2` (`opt -O2 -S`
collapses to just `ret void`, annotation still lost).

## Notes

This is a different deletion site from w396 (end-of-function) and w395
(partial merge): the terminator branch deletes the **dead** store at line
2666 rather than the killing store. It is a third independent place where
non-debug metadata is dropped, suggesting the root fix belongs in
`deleteDeadInstruction` rather than at each call site.
