# MachineLICM: IsGuaranteedToExecute only checks dominators of exiting blocks

File: `llvm/lib/CodeGen/MachineLICM.cpp:751-769` (`IsGuaranteedToExecute`)
       called from `:1093-1097` (load speculation gate) and `:1330` (LICM
       cheap-instr gate).

## Reasoning

```cpp
bool MachineLICMImpl::IsGuaranteedToExecute(MachineBasicBlock *BB,
                                            MachineLoop *CurLoop) {
  if (SpeculationState != SpeculateUnknown)
    return SpeculationState == SpeculateFalse;

  if (BB != CurLoop->getHeader()) {
    SmallVector<MachineBasicBlock*, 8> CurrentLoopExitingBlocks;
    CurLoop->getExitingBlocks(CurrentLoopExitingBlocks);
    for (MachineBasicBlock *CurrentLoopExitingBlock : CurrentLoopExitingBlocks)
      if (!MDTU->getDomTree().dominates(BB, CurrentLoopExitingBlock)) {
        SpeculationState = SpeculateTrue;
        return false;
      }
  }
  ...
  return true;
}
```

The check considers "executes on every iteration" equivalent to "dominates
every exiting block". This is incorrect when:

1. The loop body contains an instruction earlier than `BB` that can **throw or
   terminate the program abnormally** without being a loop-exit branch.
   On x86 MIR after isel, the most common case is `INLINEASM` with side
   effects or an instruction that may trap (e.g. a divide). Such an
   instruction does not appear as an exiting branch and so `getExitingBlocks`
   misses it.

2. A `mayLoad` instruction in `BB`'s dominator path that is **not** ordered
   memory but may trap (segfault). `mayLoad && !isInvariantLoad` is rejected
   only at line 1093 *for the candidate*; for the *dominator check itself*,
   earlier potentially-faulting instructions are not considered.

The result: a load that the LICM caller (`IsLICMCandidate`) reaches at
line 1093 will be hoisted out of the loop's preheader if `IsGuaranteedToExecute(I.getParent())`
returns true, even though execution may not reach that block on every
iteration due to an earlier potentially-trapping instruction.

## Concrete example

```llvm
define void @f(ptr %p, ptr %q, i32 %n) {
entry: br label %loop
loop:
  %i = phi i32 [0,%entry],[%inc,%body]
  call void asm sideeffect "test %0", "r,~{memory}"(ptr %q)  ; can fault
  br label %body
body:                                                         ; dominates exit
  %v = load i32, ptr %p                                       ; LICM candidate
  %inc = add i32 %i, 1
  %c = icmp slt i32 %inc, %n
  br i1 %c, label %loop, label %exit
exit: ret void
}
```

`%body` dominates the only exiting block, so `IsGuaranteedToExecute(%body)` is
true; the load gets hoisted into preheader. But the inline-asm in `%loop` may
fault on iteration k, so the load was *not* actually guaranteed to execute on
that iteration. Hoisting it speculates the load past the fault. If `%p` was
about to be freed or made invalid by the fault handler, this is a miscompile.

For pointer-to-pointer scenarios where `%p` is only valid after the asm
succeeds, the hoist is a clear miscompile.

## Expected wrong outcome

`llc -O2 -mtriple=x86_64` on the above IR will emit the `mov (%rdi), %eax`
in the loop preheader rather than inside the loop body, observable via
`-print-after=machinelicm`. Whether this causes an observable segfault
depends on the specific runtime, but the speculation is illegal.

## Note

This is a long-standing structural limit of MachineLICM (mirrored from the
IR LICM `isGuaranteedToTransferExecutionToSuccessor` chain). The mid-end
LICM in `lib/Transforms/Scalar/LICM.cpp` walks the basic blocks looking
for `mayThrow` (`IsGuaranteedToTransferExecutionToSuccessor`); the
MachineLICM equivalent does not.
