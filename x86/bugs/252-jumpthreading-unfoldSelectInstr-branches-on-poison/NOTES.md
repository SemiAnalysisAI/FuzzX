# 252 — JumpThreading `unfoldSelectInstr` creates `br i1 SI->getCondition()` without freeze — soundness regression

Component: `llvm/lib/Transforms/Scalar/JumpThreading.cpp` line ~2792 (`unfoldSelectInstr`)

When converting a `select` into a conditional branch by threading through a phi, the new branch directly uses `SI->getCondition()` with no freeze guard. The sibling `tryToUnfoldSelectInCurrBB` (line 2986) correctly inserts a freeze when needed; `unfoldSelectInstr` does not.

Per LangRef, `select i1 poison, ...` is well-defined (returns poison value). But `br i1 poison, ...` is immediate UB. Converting the former to the latter is a soundness regression.

## Reproducer

```ll
define void @test(i1 %c, i1 %maybe_poison) {
entry:
  br i1 %c, label %pred1, label %pred2
pred1:
  %s = select i1 %maybe_poison, i32 0, i32 10
  br label %merge
pred2:
  br label %merge
merge:
  %p = phi i32 [ %s, %pred1 ], [ 5, %pred2 ]
  %cmp = icmp eq i32 %p, 0
  %fcmp = freeze i1 %cmp
  br i1 %fcmp, label %if_then, label %if_else
if_then: call void @sink(i32 1) ret void
if_else: call void @sink(i32 2) ret void
}
```

`opt -passes=jump-threading -S`:
- Input: safely uses `freeze` to consume potentially-poison `%cmp` before branching.
- Output: JT eliminates the freeze and emits `br i1 %maybe_poison, label %if_then, label %if_else` — direct branch on `%maybe_poison` which may be poison → immediate UB.

## Severity

Default x86 -O2. Soundness regression — turns well-defined code into UB-triggering code.

## Fix

Mirror `tryToUnfoldSelectInCurrBB`'s pattern: when threading through the select, freeze the condition before using it in the new branch.
