# w421 — `foldTwoEntryPHINode` (via `hoistAllInstructionsInto`) drops `!tbaa`, `!nontemporal`, `!alias.scope`, `!noalias`, `!invariant.load`, `!invariant.group`, `!access_group`, `!mem_parallel_loop_access`

Severity: missed optimization (loss of AA / vectorizer / cache-hint metadata).
Not a miscompile.

## Where

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp:3834` calls
`hoistAllInstructionsInto`:

```cpp
// Move all 'aggressive' instructions, which are defined in the
// conditional parts of the if's up to the dominating block.
for (BasicBlock *IfBlock : IfBlocks)
    hoistAllInstructionsInto(DomBlock, DomBI, IfBlock);
```

Helper at `llvm/lib/Transforms/Utils/Local.cpp:3392`:

```cpp
void llvm::hoistAllInstructionsInto(BasicBlock *DomBlock, Instruction *InsertPt,
                                    BasicBlock *BB) {
  ...
  for (BasicBlock::iterator II = BB->begin(), IE = BB->end(); II != IE;) {
    Instruction *I = &*II;
    I->dropUBImplyingAttrsAndMetadata();    // <-- this line
    ...
  }
  DomBlock->splice(InsertPt->getIterator(), BB, BB->begin(),
                   BB->getTerminator()->getIterator());
}
```

## What's wrong

When converting a 2-entry PHI to a select, `foldTwoEntryPHINode` calls
`hoistAllInstructionsInto` on each conditional `IfBlock`. That helper invokes
`Instruction::dropUBImplyingAttrsAndMetadata()` on every instruction before
splicing it into the dominating block. That helper keeps only six metadata
kinds (`!annotation`, `!range`, `!nonnull`, `!align`, `!fpmath`, `!prof`) and
drops everything else. As a result, every hoisted instruction loses:

- `!tbaa` (AA hint)
- `!nontemporal` (cache hint)
- `!invariant.load`, `!invariant.group`
- `!access_group`, `!mem_parallel_loop_access`
- `!memprof`, `!callsite`, `!callees`, `!callee_type` (on calls)

The same `dropUBImplyingAttrsAndMetadata` issue is the root cause of candidate
w420 (`speculativelyExecuteBB`); but the trigger condition is distinct — w421
fires whenever a two-entry PHI is collapsed into a select, which is a much
more common path in default `-passes=simplifycfg`.

## Reproducer

`/tmp/w420/t38_fold2entry_simpler.ll`:

```ll
target datalayout = "e-m:e-p:64:64-i64:64-v128:128:128-a:0:64-S64"
target triple = "x86_64-unknown-linux-gnu"

declare i32 @llvm.smin.i32(i32, i32) nounwind readnone willreturn

define i32 @f(i1 %c, i32 %x, i32 %y) {
entry:
  br i1 %c, label %if, label %else
if:
  %v1 = call i32 @llvm.smin.i32(i32 %x, i32 100), !tbaa !0
  br label %end
else:
  %v2 = add nsw i32 %y, 1
  br label %end
end:
  %r = phi i32 [ %v1, %if ], [ %v2, %else ]
  ret i32 %r
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C/C++ TBAA"}
```

Pipeline confirmed default: `opt -passes=simplifycfg -S`. No non-default
SimplifyCFG option needed.

After:

```ll
define i32 @f(i1 %c, i32 %x, i32 %y) {
entry:
  %v1 = call i32 @llvm.smin.i32(i32 %x, i32 100)   ; !tbaa GONE
  %v2 = add nsw i32 %y, 1                          ; nsw preserved (IR flag, not metadata)
  %r = select i1 %c, i32 %v1, i32 %v2
  ret i32 %r
}
```

Note that `nsw` (an IR flag, stored in `SubclassOptionalData`) survives —
because `dropUBImplyingAttrsAndMetadata` only operates on metadata and
CallBase attributes, not on instruction flags. So the bug is specifically
about metadata loss; flags are not affected.

## Severity / class

Loss of optimization metadata, propagated to every later AA/LICM/vectorizer
pass that runs after the first `simplifycfg`. In O2 pipelines `simplifycfg` is
invoked very early and many times — so `!tbaa` and `!invariant.load` dropped
here typically *stay* dropped through the rest of the pipeline.

## Notes

- Suggested fix: same as w420 — broaden the keep-list in
  `dropUBImplyingAttrsAndMetadata`, or have `hoistAllInstructionsInto` pass an
  explicit `Keep` array enumerating non-UB-implying kinds.
- The `dropLocation()` issue is separate and documented in PR39141.
- Also note: the helper's comment ("Strip all UB-implying metadata") is
  misleading; the actual behavior is "strip *all* metadata except a tiny
  allowlist of 6 kinds".
- An almost-identical regression in `hoistConditionalLoadsStores`
  (`SimplifyCFG.cpp:1815`) at least preserves `!range` by transferring it to
  the masked load's range attribute; the speculate/hoist paths do not.
