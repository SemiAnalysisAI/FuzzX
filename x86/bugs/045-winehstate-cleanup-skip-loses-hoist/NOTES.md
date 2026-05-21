# X86WinEHState: cleanup-pad block skipped in the state-insertion loop loses the FinalState-hoist for normal control-flow

**File:** `llvm/lib/Target/X86/X86WinEHState.cpp:779-805`

## Reasoning
The final state-store-emit loop (line 779) iterates RPOT and skips any block whose funclet entry is a `CleanupPadInst`:

```cpp
for (BasicBlock *BB : RPOT) {
  auto &BBColors = BlockColors[BB];
  BasicBlock *FuncletEntryBB = BBColors.front();
  if (isa<CleanupPadInst>(FuncletEntryBB->getFirstNonPHIIt()))
    continue;
  ...
}
```

But `FinalStates[BB]` has already been hoisted at line 766-775 to reflect the COMMON successor `InitialState` (so the parent can avoid redundant state stores at the call-site by relying on the cleanup block's terminator carrying the right state). Inside a cleanup the runtime state isn't directly observable, fine — but the block list iterated at line 779 is the same RPOT used for non-cleanup hoist propagation.

When PrevState for a successor `BB_succ` is computed via `getPredState(FinalStates, F, ParentBaseState, BB_succ)` (line 785) and one of `BB_succ`'s predecessors is the cleanup block, `FinalStates[cleanup_BB]` may have been set to `getSuccState(...)` reflecting the successors of the cleanup. For a `cleanupret` exit, the cleanup's successor in IR is the unwind dest, but normal `cleanupret` to an outer cleanup doesn't return to the parent CFG.

The bigger issue is in `getPredState` itself (line 595-632): it does NOT special-case the case where the predecessor is a cleanup-pad block. A non-cleanup BB whose only predecessor is a cleanup-pad block will inherit `FinalStates[cleanup]` (which was inserted by the hoist at 774) as `PrevState`, and then the state-store-emit loop will use that PrevState to decide whether the first call needs a store. If the hoisted `FinalStates[cleanup]` was set by the SuccState-from-cleanup-successor logic, this is fine; but if the cleanup block has no entry in `FinalStates` because the worklist-fill never reached it (cleanups are EH pads, line 603 returns OverdefinedState for predecessor lookups, line 652 returns Overdefined for successor lookups), the lookup at line 785 falls back to `ParentBaseState` (-1) — even when the actual dynamic state at the joinpoint is a real numbered TryLevel.

The end effect: an unnecessary state store of `-1` is emitted on the path leaving a cleanup, demoting TryLevel to -1, then the cleanup unwinds and the parent personality observes the wrong state → wrong destructors / wrong catch entered.

## IR/MIR repro sketch
```
; opt -passes=x86-winehstate -S t.ll | llc -mtriple=i686-pc-windows-msvc
define i32 @f() personality ptr @__CxxFrameHandler3 {
entry:
  invoke void @may_throw() to label %try.cont unwind label %catch.dispatch
catch.dispatch:
  %cs = catchswitch within none [label %catch] unwind to caller
catch:
  %cp = catchpad within %cs [ptr null, i32 64, ptr null]
  invoke void @inner() to label %catch.exit unwind label %cleanup
catch.exit:
  catchret from %cp to label %try.cont
cleanup:
  %cu = cleanuppad within %cp []
  call void @cleanup_body()
  cleanupret from %cu unwind to caller
try.cont:
  ret i32 0
}
```
Expected wrong outcome: state store before `@cleanup_body` is `-1` rather than the catch's TryLevel, so an exception thrown from `@cleanup_body` would unwind past the catch's destructors instead of running them.
