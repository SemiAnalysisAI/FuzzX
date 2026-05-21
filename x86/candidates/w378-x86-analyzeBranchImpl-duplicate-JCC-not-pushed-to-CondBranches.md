# w378: X86InstrInfo::analyzeBranchImpl skips pushing duplicate-condition conditional branches to `CondBranches` vector

## Component
`llvm/lib/Target/X86/X86InstrInfo.cpp` - `X86InstrInfo::analyzeBranchImpl`.

## Where
- `llvm/lib/Target/X86/X86InstrInfo.cpp:3870-3898`

```cpp
3870    X86::CondCode BranchCode = X86::getCondFromBranch(*I);
...
3880    if (Cond.empty()) {
3881      FBB = TBB;
3882      TBB = I->getOperand(0).getMBB();
3883      Cond.push_back(MachineOperand::CreateImm(BranchCode));
3884      CondBranches.push_back(&*I);
3885      continue;
3886    }
...
3895    X86::CondCode OldBranchCode = (X86::CondCode)Cond[0].getImm();
3896    auto NewTBB = I->getOperand(0).getMBB();
3897    if (OldBranchCode == BranchCode && TBB == NewTBB)
3898      continue;
```

## Bug
When the bottom-up walk encounters a second `Jcc` whose condition and target both match the first `Jcc`, the code `continue`s without pushing the second branch into `CondBranches`. This is the "two identical JCCs to same target" case (effectively a redundant branch).

`CondBranches` is consumed by callers to update / remove conditional branches. For example, `analyzeBranchPredicate` (line 4011-4070) and `removeBranch` semantics depend on enumerating all conditional branches. With this skip, the redundant second JCC remains in the basic block uncatalogued. Downstream callers that walk `CondBranches` to update or rewrite all conditional branches will miss the duplicate, leaving stale unreachable-but-physically-present code in the MBB.

The classic patterns the function does handle correctly are at 3903-3933 (`COND_NE_OR_P`, `COND_E_AND_NP`) - those *do* push the second branch via line 3937 `CondBranches.push_back(&*I);`. Only the exact-duplicate continue at 3897 is asymmetric.

## Impact
Two scenarios:
1. `BranchFolding`/`MachineBlockPlacement` reorders MBBs and inserts new branches. If two identical Jccs already exist (e.g., produced by a tail-merging or `if-conversion` reshape), `analyzeBranch` returns success with `Cond.size() == 1` and `CondBranches` containing only the *first* (== bottom-most). `insertBranch` then re-emits one Jcc as if there were one. The duplicate Jcc above it is never removed.
2. The duplicate sits in the function, costs code-size, and (more concerning) if the dead-but-physically-present second Jcc happens to occupy a different layout slot than analysis predicted, callers that rely on `MBB.terminators()` count may go out of sync with the post-analyze view.

The function also exits at line 3940 with `return false` (success) regardless of whether `CondBranches.size()` reflects all in-block conditional branches.

## Repro hypothesis
Triggering needs a basic block ending with two identical conditional branches to the same target plus a fall-through - rarely produced by isel directly, but reachable after:
- A tail-merge that merges two identical predecessors' last JCC.
- A pattern where MBB-layout duplication causes the same Jcc to be emitted twice.

A speculative IR pattern:

```ll
target triple = "x86_64-unknown-linux-gnu"

define void @dup(i32 %x, ptr %p) {
entry:
  %c = icmp slt i32 %x, 0
  br i1 %c, label %t, label %m
m:
  store i32 0, ptr %p
  br i1 %c, label %t, label %t
t:
  store i32 1, ptr %p
  ret void
}
```

Default `llc -O2 -mtriple=x86_64-unknown-linux-gnu` will typically simplify this earlier (CFGSimplify / branchfolding) and never reach the duplicate-JCC case visible to `analyzeBranch`. To exercise the X86-specific path you would need to inject duplicates after `-stop-before=branch-folder` and re-feed.

## Why I flag it anyway
- The user listed `analyzeBranch wrong predicate inversion on Test* instructions` as a target. This case is *not* an inversion bug, but it is an `analyzeBranch` *omission* with consistent symptoms (post-analyzeBranch view of the MBB undercounts conditional branches), and on a fuzzer the likely manifestation is `assert(I->isBranch())` or block-count divergence in branch folding.
- The duplicate-JCC path being silently dropped *and* returning `false` (success) is the kind of bug a fuzzer could trip if it manages to construct overlapping Jccs via pattern reductions.

## Fix sketch
Push the second branch into `CondBranches` even when it is a duplicate, OR delete the duplicate JCC in place when `AllowModify`:

```cpp
if (OldBranchCode == BranchCode && TBB == NewTBB) {
  if (AllowModify) {
    MachineBasicBlock::iterator next = std::next(I);
    I->eraseFromParent();
    I = next;  // reset iterator carefully
  } else {
    CondBranches.push_back(&*I);
  }
  continue;
}
```

## Confidence
Medium. The omission is real and asymmetric vs. the `COND_NE_OR_P`/`COND_E_AND_NP` paths; observable misbehavior depends on a downstream caller iterating `CondBranches` for completeness.
