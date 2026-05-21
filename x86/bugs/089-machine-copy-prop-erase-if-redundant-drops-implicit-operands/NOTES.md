# MachineCopyPropagation::eraseIfRedundant drops implicit operands

File: llvm/lib/CodeGen/MachineCopyPropagation.cpp, function
`MachineCopyPropagation::eraseIfRedundant` (lines 624-664), called from
`forwardCopyPropagateBlock` (line 942).

## Pattern

```
COPY %rax = %rbx                                ; (A) tracked first
COPY %rax = %rbx, implicit-def %rcx             ; (B) NopCopy w.r.t. (A) - ERASED
```

`eraseIfRedundant` only checks:
- `isNeverRedundant` (FrameSetup/FrameDestroy MI flag; reserved src/dst regs)
- `findAvailCopy` (returns PrevCopy whose Dst/Src match the same physreg units)
- `isNopCopy` (compares only Src and Dst registers / subreg indices)
- `PrevCopyOperands.Destination->isDead()` (skip if the prior dst is dead)

Critically, it does **not** inspect `Copy.getNumImplicitOperands()` nor compare
the implicit-def/use sets of the two copies. `Copy.eraseFromParent()` (line 660)
removes the second instruction outright, silently dropping any `implicit-def`
operands that (B) carries but (A) does not.

The backward path (`backwardCopyPropagateBlock`, line 1183) and the spillage path
(`GetFoldableCopy`, line 1399) both guard with
`MI.getNumImplicitOperands() == 0`. The forward `eraseIfRedundant` has no such
guard.

## Why this matters

Implicit-defs on COPY-like moves are not just decorative — they appear in real
post-RA MIR. Concrete sources observed in the codebase:

- `coalesce-commutative-tied-def-subreg.mir` carries
  `AND32rr ..., implicit-def dead $eflags, implicit-def %1` where the
  super-register def is recorded as an implicit operand of the narrower
  arithmetic.
- Targets sometimes use a register-class-changing COPY plus an
  `implicit-def` of the wider super-register to model a zero-extending move
  semantically (so that post-RA passes treat the high bits as defined).
- ARM emits `tBX_RET ..., implicit-def $sp` style copies in some lowerings;
  while not x86, this pass is target-agnostic and ARM/AArch64 are equally
  affected.

If (A) does not include the `implicit-def %rcx` annotation and (B) does, after
erasing (B) the only place where %rcx was defined is gone. Subsequent uses of
%rcx (live-out of the block, in successor MBBs, or further in this MBB beyond
where the tracker can reason about — note the tracker is per-BB) will read
whatever value happened to be in %rcx prior to this code, which is uninitialized.

Note that the symmetric `eraseIfRedundant(MI, Src, Dst)` swap on line 942 (when
e.g. `COPY %rax = %rbx ; %rbx = COPY %rax`) ALSO hits this path with the same
gap.

## What a correct fix looks like

The fix mirrors the backward/spillage gating: bail when
`Copy.getNumImplicitOperands() > 0`, or compare the implicit-operand set with
PrevCopy and either copy the implicit defs onto PrevCopy or refuse the
elimination.

## Reproduction sketch (MIR-level, not yet executed)

```
bb.0:
  $rbx = MOV64ri 0
  $rcx = MOV64ri 0
  renamable $rax = COPY renamable $rbx
  renamable $rax = COPY renamable $rbx, implicit-def $rcx
  ; use $rax and $rcx
  ...
```

Run with `llc -run-pass=machine-cp -mtriple=x86_64-linux-gnu`. Expected: at
least one COPY survives that defines $rcx (either by keeping the second copy
or by hoisting `implicit-def $rcx` onto the first). Observed (per source
reading): the second copy is erased outright, leaving $rcx undefined at
subsequent uses.

## Confidence

Medium. The code path is clearly missing the implicit-operand guard that the
analogous paths have, and that omission is the kind of latent correctness bug
the fuzzer is meant to catch. I have not constructed a complete miscompiling
input — the difficulty is that LLVM's *own* generators rarely emit a
`COPY ..., implicit-def $foo` for a redundant move, so the trigger requires
either a target that does, an inline-MIR test, or coalescing/outliner output
that lands such pairs. The structural gap is unambiguous in the source.
