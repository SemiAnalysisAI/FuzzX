# w336: SimpleLoopUnswitch trivial branch unswitch keeps !make.implicit on the hoisted branch (no safety check)

Pass: `simple-loop-unswitch` (trivial path, runs in default x86 -O2)
File: `llvm/lib/Transforms/Scalar/SimpleLoopUnswitch.cpp`
Function: `unswitchTrivialBranch`

## Root cause

In `unswitchTrivialBranch` the full-unswitch path moves the original conditional branch out of the loop body and into the (split) preheader via `BI.moveBefore(*OldPH, OldPH->end())` (line 685). Because the branch instruction object is moved (not cloned), every metadata kind travels with it — including `!make.implicit`.

```cpp
// SimpleLoopUnswitch.cpp:680-698
OldPH->getTerminator()->eraseFromParent();
if (FullUnswitch) {
  // If fully unswitching, we can use the existing branch instruction.
  // Splice it into the old PH to gate reaching the new preheader and re-point
  // its successors.
  BI.moveBefore(*OldPH, OldPH->end());
  BI.setCondition(Cond);
  if (MSSAU) {
    // Temporarily clone the terminator, to make MSSA update cheaper by
    // separating "insert edge" updates from "remove edge" ones.
    BI.clone()->insertInto(ParentBB, ParentBB->end());
  } else {
    Instruction *NewBI = UncondBrInst::Create(ContinueBB, ParentBB);
    NewBI->setDebugLoc(BI.getDebugLoc());
  }
  BI.setSuccessor(LoopExitSuccIdx, UnswitchedBB);
  BI.setSuccessor(1 - LoopExitSuccIdx, NewPH);
```

`!make.implicit` is the explicit-null-check marker: the conditional branch must immediately follow the load it was synthesized from so the implicit-null-check pass can fold them into a single faulting instruction. After hoisting the branch into the preheader, the load that produced the marker is no longer adjacent — and may not even be on the path that reaches the branch (the load is still inside the loop body; the preheader executes the branch before any iteration runs).

The non-trivial unswitch path explicitly handles this exact case at SimpleLoopUnswitch.cpp:2370-2385:

```cpp
// Drop metadata if we may break its semantics by moving this instr into the
// split block.
if (TI.getMetadata(LLVMContext::MD_make_implicit)) {
  if (DropNonTrivialImplicitNullChecks)
    TI.setMetadata(LLVMContext::MD_make_implicit, nullptr);
  else {
    ICFLoopSafetyInfo SafetyInfo;
    SafetyInfo.computeLoopSafetyInfo(&L);
    if (!SafetyInfo.isGuaranteedToExecute(TI, &DT, &L))
      TI.setMetadata(LLVMContext::MD_make_implicit, nullptr);
  }
}
```

The trivial path has no equivalent — `!make.implicit` is simply moved out together with the rest of `BI`.

## Reproducer

```llvm
; opt -passes='simple-loop-unswitch<no-nontrivial;trivial>' -S
define void @f(ptr %p, i32 %n, i1 %c) {
entry:
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %inc, %backedge ]
  br i1 %c, label %exit_trap, label %backedge, !make.implicit !0

exit_trap:
  ret void

backedge:
  %inc = add i32 %i, 1
  %cmp = icmp slt i32 %inc, %n
  br i1 %cmp, label %loop, label %exit

exit:
  ret void
}
!0 = !{}
```

## Diff

Before:
- In `%loop`: `br i1 %c, label %exit_trap, label %backedge, !make.implicit !0`

After (trivial unswitch hoists the conditional branch into entry):
- In `%entry`:   `br i1 %c, label %exit_trap, label %entry.split, !make.implicit !0`
- In `%loop`:    `br label %backedge` (unconditional, no marker)

The marker now sits on a branch in the preheader that is no longer preceded by any faulting memory operation. The implicit-null-check pass at codegen time still sees a `!make.implicit` branch and will look for an adjacent dereferenceable load to fold — which won't be there.

## Impact / why it matters in -O2

In default -O2 the trivial unswitch path runs (`simple-loop-unswitch<no-nontrivial;trivial>`). Implicit null check synthesis is normally driven by frontends/ManagedRuntimes (e.g. via `-implicit-null-checks`) but the metadata sticks once placed. With this bug, the metadata reaches the late `implicit-null-checks` pass attached to the wrong branch, where it can:
- prevent fold opportunities (silent), or
- mis-fold an unrelated nearby memory operation into a fault handler (correctness).

## Suggested fix

After `BI.moveBefore(*OldPH, OldPH->end())`, mirror the non-trivial path's handling: clear `MD_make_implicit` on `BI` if it would no longer be guaranteed-to-execute at the hoisted position (and, symmetrically, respect `DropNonTrivialImplicitNullChecks`). The branch in the preheader runs unconditionally on loop entry, so it is trivially not preceded by the original guarding load — the metadata can be dropped unconditionally.
