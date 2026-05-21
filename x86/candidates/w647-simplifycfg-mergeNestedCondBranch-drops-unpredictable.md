# w647 - SimplifyCFG `mergeNestedCondBranch` drops `!unpredictable` from inner branches

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` lines 8482-8549
(`mergeNestedCondBranch`). Called unconditionally from
`SimplifyCFGOpt::simplifyCondBranch` at line 8687 in the default
pass-spec.

The function recognises:

```
entry: br i1 %c1, label %bb1, label %bb2
bb1:   br i1 %c2, label %bb3, label %bb4    ; possibly with !unpredictable
bb2:   br i1 %c2, label %bb4, label %bb3    ; possibly with !unpredictable
```

and rewrites `entry`'s branch to `br i1 (xor %c1, %c2), label %bb4, label %bb3`
(lines 8511-8516). Branch weights are merged into the new combined branch
(lines 8526-8547), but no other metadata is preserved. `BB1BI` and `BB2BI`
are then erased by their owning blocks losing predecessors, and any
`!unpredictable` / `!annotation` / `!nosanitize` they carried is gone.

```cpp
IRBuilder<> Builder(BI);
BI->setCondition(
    Builder.CreateXor(BI->getCondition(), BB1BI->getCondition()));
BB1->removePredecessor(BB);
BI->setSuccessor(0, BB4);
BB2->removePredecessor(BB);
BI->setSuccessor(1, BB3);
...
// only branch weights are merged below; no MD_unpredictable propagation
```

This is a metadata correctness regression: the source IR explicitly
declared the inner branches as unpredictable (e.g. `__builtin_unpredictable`
in C), and after this fold neither the outer XOR-merged branch nor any
descendant of it carries the hint.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define void @nested_cond(i1 %c1, i1 %c2, ptr %p) {
entry:
  br i1 %c1, label %bb1, label %bb2
bb1:
  br i1 %c2, label %bb3, label %bb4, !unpredictable !0
bb2:
  br i1 %c2, label %bb4, label %bb3, !unpredictable !0
bb3:
  store i32 1, ptr %p
  ret void
bb4:
  store i32 2, ptr %p
  ret void
}

!0 = !{}
```

## Invocation

```
opt -passes=simplifycfg -S repro.ll
```

## Observed output

```
define void @nested_cond(i1 %c1, i1 %c2, ptr %p) {
entry:
  %0 = xor i1 %c1, %c2
  br i1 %0, label %bb4, label %bb3                  ; <-- no !unpredictable
...
}
```

The merged XOR branch is exactly the conjunction the inner branches were
controlling, and the source authoritatively said "this is unpredictable" —
yet the rewritten branch has no metadata at all.

## Fix

Before rewriting `BI->setCondition(...)`, capture the unpredictability
hint and re-apply it:

```cpp
bool Unpred = BB1BI->getMetadata(LLVMContext::MD_unpredictable) ||
              BB2BI->getMetadata(LLVMContext::MD_unpredictable) ||
              BI->getMetadata(LLVMContext::MD_unpredictable);
...
BI->setCondition(
    Builder.CreateXor(BI->getCondition(), BB1BI->getCondition()));
...
if (Unpred)
  BI->setMetadata(LLVMContext::MD_unpredictable,
                  MDNode::get(BI->getContext(), {}));
```

The "either inner branch was unpredictable ⇒ the merged xor branch is
unpredictable" rule is correct: if either of the inner branches has no
useful predictor, the xor combination has none either.
