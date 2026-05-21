# JumpThreading duplicateCondBranchOnPHIIntoPred does not clone noalias scopes (unlike cloneInstructions)

## Location
`llvm/lib/Transforms/Scalar/JumpThreading.cpp:2682-2738` inside
`JumpThreadingPass::duplicateCondBranchOnPHIIntoPred`.

The function clones the contents of BB into PredBB with its own inline loop:

```cpp
for (; BI != BB->end(); ++BI) {
  Instruction *New = BI->clone();
  New->insertInto(PredBB, OldPredBranch->getIterator());
  // ... remap intra-block operands, run simplifyInstruction ...
}
```

## Bug
Compare with `JumpThreadingPass::cloneInstructions` (line 2037), used by
`threadEdge` and `threadThroughTwoBasicBlocks`, which DOES clone noalias
scopes for the duplicated instructions:

```cpp
SmallVector<MDNode *> NoAliasScopes;
DenseMap<MDNode *, MDNode *> ClonedScopes;
LLVMContext &Context = PredBB->getContext();
identifyNoAliasScopesToClone(BI, BE, NoAliasScopes);
cloneNoAliasScopes(NoAliasScopes, ClonedScopes, "thread", Context);
// ... and inside the clone loop:
adaptNoAliasScopes(New, ClonedScopes, Context);
```

`duplicateCondBranchOnPHIIntoPred` has no equivalent. Therefore the cloned
load/store in PredBB carries the **same** `!alias.scope` / `!noalias`
MDNodes as the original in BB. Per the inliner / `noalias` semantics, two
distinct dynamic instances of an inlined `noalias` argument get distinct
scopes (that is what `cloneNoAliasScopes` exists to materialise); after
JumpThreading duplicates the block, the duplicated instance must NOT share
the original's scope, or AA can conclude two accesses on different paths
don't alias when they should.

## Reproducer
`/tmp/w85/nas3.ll`:
```llvm
target triple = "x86_64-unknown-linux-gnu"
declare i1 @opaque1()
define i32 @f(ptr noalias %p, ptr noalias %q, i32 %a, i32 %b) {
entry:
  %c = icmp slt i32 %a, %b
  br i1 %c, label %pred, label %other
pred:
  br label %bb
other:
  %o = call i1 @opaque1()
  br label %bb
bb:
  %x = phi i1 [ true, %pred ], [ %o, %other ]
  %v1 = load i32, ptr %p, !alias.scope !1
  store i32 %v1, ptr %q, !noalias !1
  br i1 %x, label %tb, label %fb
tb:
  ret i32 1
fb:
  ret i32 2
}
!0 = !{!"root"}
!1 = !{!2}
!2 = !{!"scope1", !0}
```

After `opt -passes=jump-threading -S`:
```llvm
entry:
  %c = icmp slt i32 %a, %b
  br i1 %c, label %bb.thread, label %bb

bb.thread:                                        ; preds = %entry
  %v12 = load i32, ptr %p, !alias.scope !0     ; <-- SAME scope MDNode
  store i32 %v12, ptr %q, !noalias !0          ; <-- SAME scope MDNode
  br label %tb

bb:                                               ; preds = %entry
  %o = call i1 @opaque1()
  %v1 = load i32, ptr %p, !alias.scope !0      ; original
  store i32 %v1, ptr %q, !noalias !0
  br i1 %o, label %tb, label %fb
...
!0 = !{!1}
!1 = !{!"scope1", !2}
```

The duplicate `load/store` in `bb.thread` shares scope `!0` with the
originals in `bb`. Compare to `threadEdge` which would emit a new scope
`!"thread:scope1"` for the duplicate.

## Severity
Latent AA mis-information. End-to-end miscompile requires a downstream
AA-consuming pass (e.g. LICM, GVN) to see both copies and act on the
incorrect "doesn't alias" conclusion. Worth filing as either:
1. a soundness bug ("`duplicateCondBranchOnPHIIntoPred` must clone
   noalias scopes like `cloneInstructions` does"), or
2. a code-cleanup refactor to have `duplicateCondBranchOnPHIIntoPred`
   call `cloneInstructions` instead of re-implementing it.

The fix is a 3-line addition mirroring the call into `cloneNoAliasScopes`
and `adaptNoAliasScopes`.

## Status
Source-confirmed + transform-confirmed (the scope MDNode is unchanged in
the duplicate). Runtime miscompile requires constructing a downstream
AA-dependent transform; left for follow-up.
