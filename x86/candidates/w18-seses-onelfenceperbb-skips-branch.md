# SESES: `-x86-seses-one-lfence-per-bb` breaks out before branch LFENCE

File: llvm/lib/Target/X86/X86SpeculativeExecutionSideEffectSuppression.cpp:124-172

```
if (MI.mayLoadOrStore() && !MI.isTerminator()) {
  if (!PrevInstIsLFENCE) {
    BuildMI(MBB, MI, DebugLoc(), TII->get(X86::LFENCE));
    NumLFENCEsInserted++;
    Modified = true;
  }
  if (OneLFENCEPerBasicBlock)
    break;          // <-- exits the per-MBB loop
}
...
// Branch handling is below the load handling but inside the same loop.
```

## Reasoning

When the user passes `-x86-seses-one-lfence-per-bb`, the intent (per
the option description) is "Omit all lfences other than the first to be
placed in a basic block." But the implementation uses `break` after
emitting the first per-BB LFENCE for a load/store, which jumps out of
the *entire* per-instruction loop. As a result the branch-LFENCE logic
further down (lines 146-171, which is what closes the *branch-prediction*
side channel) never runs for that basic block.

This is asymmetric: if a block has a load followed by a branch, only
the load gets an LFENCE; the branch gets none. If the block has only
a branch (no loads/stores), the branch LFENCE is correctly emitted.
The option was intended to deduplicate redundant LFENCEs within a
block, not to disable the branch-LFENCE pass entirely whenever a load
exists in the block. The `PrevInstIsLFENCE` tracking immediately above
(lines 112-116, plus the recheck on line 165) is the correct
deduplication mechanism; the `break` is the bug.

Worse, on the security side: the documented purpose of the branch
LFENCE is to close the BTB/PHT-trained misspeculation channel. After
this `break`, that mitigation is silently dropped for every block
containing at least one memory access — which is most blocks. The
attacker model SESES is meant to defeat (Spectre-v1/v2 misspeculation
fanning out from a branch) is left fully exploitable.

The intended behavior is to `continue` (i.e. just don't emit a *second*
LFENCE in this block), not `break`. The `PrevInstIsLFENCE = true` set
on line 116 (when we observe the LFENCE we just inserted on the next
iteration) is already the correct dedup mechanism.

## Repro sketch

```
; llc -mtriple=x86_64-linux-gnu \
;   -mllvm -x86-seses-enable-without-lvi-cfi \
;   -mllvm -x86-seses-one-lfence-per-bb \
;   reduce.ll
define i32 @f(ptr %p, i32 %x) {
  %v = load i32, ptr %p           ; gets an LFENCE before it
  %c = icmp slt i32 %v, 0
  br i1 %c, label %T, label %F    ; *no* LFENCE before this branch (bug)
T: ret i32 1
F: ret i32 0
}
```

## Expected wrong outcome

Generated asm shows exactly one `lfence` (before the load) and *no*
`lfence` before the `jl` terminator, despite the pass advertising
"speculative execution side-effect suppression." Compared to running
the same input without `-x86-seses-one-lfence-per-bb`, the branch
LFENCE has been silently dropped.
