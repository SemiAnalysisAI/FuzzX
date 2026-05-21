# w261: JumpThreading `unfoldSelectInstr` only copies `MD_prof`, drops `!unpredictable`

## Pass
`-passes=jump-threading` (default x86 -O2 pipeline includes this).

## Summary

`unfoldSelectInstr` (called from both `tryToUnfoldSelect(SwitchInst*)` and
`tryToUnfoldSelect(CmpInst*)`) builds a new conditional branch out of a
`select`. The body of `unfoldSelectInstr` explicitly limits the metadata
it carries over to `MD_prof`:

```cpp
// JumpThreading.cpp:2792
auto *BI = CondBrInst::Create(SI->getCondition(), NewBB, BB, Pred);
BI->applyMergedLocation(PredTerm->getDebugLoc(), SI->getDebugLoc());
BI->copyMetadata(*SI, {LLVMContext::MD_prof});       // <-- ONLY MD_prof
```

`!unpredictable` (and `!annotation`, if present) on the original `select`
is dropped on the floor. There is no `getMetadata(MD_unpredictable)` or
`copyMetadata(*SI, {MD_prof, MD_unpredictable})` anywhere on that path.

## Reproducer

Input `final_b.ll`:
```llvm
target triple = "x86_64-unknown-linux-gnu"
declare void @sink(i32)

define void @test_unfoldSelect(i1 %x, i32 %y) {
entry:
  %cmp0 = icmp eq i32 %y, 5
  br i1 %cmp0, label %bb1, label %bb2
bb1:
  %sel = select i1 %x, i32 0, i32 %y, !prof !0, !unpredictable !1
  br label %bb2
bb2:
  %p = phi i32 [ %sel, %bb1 ], [ 0, %entry ]
  %c = icmp eq i32 %p, 0
  br i1 %c, label %T, label %F
T:
  call void @sink(i32 1)
  ret void
F:
  call void @sink(i32 2)
  ret void
}
!0 = !{!"branch_weights", i32 99, i32 1}
!1 = !{}
```

Command:
```
opt -passes=jump-threading -S final_b.ll
```

Output (excerpt):
```llvm
bb1:                                              ; preds = %entry
  br i1 %x, label %T, label %F, !prof !0          ; <-- !prof kept, !unpredictable lost
```

The new conditional branch on `%x` carries `!prof !0` (forwarded from the
select's `!prof`) but **no `!unpredictable`**, even though the source
select had `!unpredictable !1`.

## Why this matters

Same as w260: `!unpredictable` flips the codegen heuristic for cmov vs.
branch in `SelectionDAGBuilder`. Losing it silently changes generated
code on x86 for any front end (e.g. `__builtin_unpredictable`) that
emits this metadata on a `select` and relies on JT not eating it.

## Suggested fix

Extend the explicit-list to include `MD_unpredictable`:
```cpp
BI->copyMetadata(*SI, {LLVMContext::MD_prof,
                       LLVMContext::MD_unpredictable});
```
A more defensive variant is to enumerate all "branch-applicable" kinds
explicitly (the same list `Instruction::dropUnknownNonDebugMetadata`
guards against).
