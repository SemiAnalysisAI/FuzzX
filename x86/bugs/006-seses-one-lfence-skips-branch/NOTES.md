# 006 — SESES: `-x86-seses-one-lfence-per-bb` silently drops branch LFENCE

Component: X86SpeculativeExecutionSideEffectSuppression

## Source

`llvm/lib/Target/X86/X86SpeculativeExecutionSideEffectSuppression.cpp:124-172`

```cpp
if (MI.mayLoadOrStore() && !MI.isTerminator()) {
  if (!PrevInstIsLFENCE) {
    BuildMI(MBB, MI, DebugLoc(), TII->get(X86::LFENCE));
    NumLFENCEsInserted++;
    Modified = true;
  }
  if (OneLFENCEPerBasicBlock)
    break;   // <-- exits the entire per-MBB loop
}
// ... branch-LFENCE handling further down ...
```

The flag is described as "Omit all lfences other than the first to be placed in
a basic block." But the `break` exits the *whole* per-instruction loop, so the
branch-LFENCE logic further down (which closes the BTB/PHT speculative
side channel) is **silently skipped** in any block that contained at least one
load/store.

Result: a function compiled with the SESES mitigation requested gets only
one LFENCE per block (before the first load/store) and zero LFENCEs before
branches. That is exactly the misspeculation channel the pass was designed
to close — so the user-facing security guarantee of SESES is broken whenever
`-x86-seses-one-lfence-per-bb` is set.

The intended deduplication mechanism is already in place: `PrevInstIsLFENCE`
is tracked across iterations and prevents back-to-back LFENCEs. The right
control-flow change is `continue`, not `break`.

## Demonstration

`repro.ll` is a load + compare + branch. `cmd.sh` shows the asm with and
without the flag:

```
===== with -x86-seses-one-lfence-per-bb (buggy: branch lfence missing) =====
lfence
cmpl $0, (%rdi)
js .LBB0_1            ; <-- no LFENCE before the conditional branch

===== without -x86-seses-one-lfence-per-bb (correct) =====
lfence
cmpl $0, (%rdi)
lfence                ; <-- correctly inserted before the branch
js .LBB0_1
```

## Severity

Mitigation regression. The pass advertises "speculative execution side-effect
suppression" but silently dropped the branch-side mitigation for every block
that touches memory — i.e., nearly every block.

## Fix

Change `break` → `continue` on the `if (OneLFENCEPerBasicBlock)` branch.
