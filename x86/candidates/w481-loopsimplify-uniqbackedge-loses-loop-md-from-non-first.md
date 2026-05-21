# LoopSimplify `insertUniqueBackedgeBlock` discards `!llvm.loop` MD from all but the first backedge

## File and root cause

`llvm/lib/Transforms/Utils/LoopSimplify.cpp:444-456` inside
`insertUniqueBackedgeBlock`.

```c++
// Now that all of the PHI nodes have been inserted and adjusted, modify the
// backedge blocks to jump to the BEBlock instead of the header.
// If one of the backedges has llvm.loop metadata attached, we remove
// it from the backedge and add it to BEBlock.
MDNode *LoopMD = nullptr;
for (BasicBlock *BB : BackedgeBlocks) {
  Instruction *TI = BB->getTerminator();
  if (!LoopMD)
    LoopMD = TI->getMetadata(LLVMContext::MD_loop);
  TI->setMetadata(LLVMContext::MD_loop, nullptr);
  TI->replaceSuccessorWith(Header, BEBlock);
}
BEBlock->getTerminator()->setMetadata(LLVMContext::MD_loop, LoopMD);
```

The comment is correct about the intent ("if one of the backedges has llvm.loop
metadata"), but the implementation is wrong for the multiple-backedges case
with **different** `!llvm.loop` metadata on each:

1. `LoopMD` is bound from the **first** backedge in iteration order whose
   terminator has `MD_loop`. After that, the conditional `if (!LoopMD)` never
   fires again.
2. The unconditional `TI->setMetadata(LLVMContext::MD_loop, nullptr);` outside
   the `if` runs on every backedge — including ones that had a *different*
   `MD_loop` we never copied to `BEBlock`.

Result: any loop-level property attached only via a non-first backedge
(`llvm.loop.unroll.count`, `llvm.loop.disable_nonforced`,
`llvm.loop.vectorize.width`, `llvm.loop.parallel_accesses`, ...) is silently
deleted by `loop-simplify` canonicalization. In practice these can land on
different backedges after pre-canonicalization loop transforms (jump
threading, unrolling that leaves multiple latches, loop fusion).

## Reproducer

`x86/candidates/w481-multi-backedge-loop-md.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @f(i32 %n) {
entry:
  br label %h

h:
  %i = phi i32 [0, %entry], [%i1, %be1], [%i2, %be2]
  %i1 = add i32 %i, 1
  %i2 = add i32 %i, 2
  %c1 = icmp ult i32 %i, %n
  br i1 %c1, label %be1, label %mid

mid:
  %c2 = icmp ult i32 %i, 100
  br i1 %c2, label %be2, label %exit

be1:
  br label %h, !llvm.loop !0       ; <-- "unroll x2 + disable_nonforced"

be2:
  br label %h, !llvm.loop !3       ; <-- "mustprogress"

exit:
  ret void
}

!0 = distinct !{!0, !1, !2}
!1 = !{!"llvm.loop.unroll.count", i32 2}
!2 = !{!"llvm.loop.disable_nonforced"}
!3 = distinct !{!3, !4}
!4 = !{!"llvm.loop.mustprogress"}
```

### `opt -passes=loop-simplify -S` actual output

```llvm
h.backedge:                                       ; preds = %be1, %be2
  %i.be = phi i32 [ %i1, %be1 ], [ %i2, %be2 ]
  br label %h, !llvm.loop !0

...

!0 = distinct !{!0, !1}
!1 = !{!"llvm.loop.mustprogress"}
```

Only `!llvm.loop.mustprogress` (the metadata from `be2`) survived on the new
unified backedge block. The original `be1`'s `llvm.loop.unroll.count=2` and
`llvm.loop.disable_nonforced` directives are silently **discarded**.

(Predecessor iteration order can pick either of the two; in this run `be2`'s
MD won. Either way, the other backedge's MD is lost.)

## Why this is a regression

* `llvm.loop.unroll.count` is a user (or `#pragma unroll`) directive. Dropping
  it means subsequent `LoopUnroll` runs without the hint and may choose a
  different unrolling than the source requested.
* `llvm.loop.disable_nonforced` is the "this loop should not be optimized
  unless explicitly forced" knob; losing it can enable transforms the user
  explicitly asked to suppress.
* `llvm.loop.parallel_accesses` underpins LoopVectorize's reordering proofs
  when paired with `!llvm.access.group` on memory ops. Losing it can cause
  vectorization to silently fail or, worse, suppress a legality check.
* `loop-simplify` is a canonicalization pass run by virtually every loop
  pipeline including the default `-O2` (and `-O1`) pipelines.

## Fix sketch

Properly merge the metadata across all backedges before zeroing them. Two
options:

1. **Union semantics:** collect every backedge's `MD_loop` and `MDNode::concatenate`
   them (or rebuild with `makePostTransformationMetadata`-style combination)
   into one loop-id node attached to BEBlock.
2. **Conservative drop-all:** if the backedges have *differing* `MD_loop`, do
   not pick a winner — drop on BEBlock too and let later code re-derive what
   it needs (least surprising; loses all hints but is at least symmetric).

Minimal correctness patch (option 2):

```c++
MDNode *LoopMD = nullptr;
bool Consistent = true;
for (BasicBlock *BB : BackedgeBlocks) {
  MDNode *MD = BB->getTerminator()->getMetadata(LLVMContext::MD_loop);
  if (!LoopMD)
    LoopMD = MD;
  else if (MD != LoopMD)
    Consistent = false;
}
for (BasicBlock *BB : BackedgeBlocks) {
  Instruction *TI = BB->getTerminator();
  TI->setMetadata(LLVMContext::MD_loop, nullptr);
  TI->replaceSuccessorWith(Header, BEBlock);
}
if (Consistent)
  BEBlock->getTerminator()->setMetadata(LLVMContext::MD_loop, LoopMD);
```

A proper union (option 1) requires building a fresh loop-id MD that
references both source nodes' properties — more invasive but lossless.
