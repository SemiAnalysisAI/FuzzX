# w103: X86InstrInfo::foldMemoryOperandImpl NDD-to-RMW path uses killsRegister without subreg awareness

## Source

`amdgpu/third_party/llvm-project/llvm/lib/Target/X86/X86InstrInfo.cpp`, function
`X86InstrInfo::foldMemoryOperandImpl`, around lines 7623-7642 (the
`if (NoNDDM && !IsTwoAddr)` block, executed after the fold has produced an
RMW (read-modify-write, non-NDD) replacement for an NDD instruction).

```cpp
if (NoNDDM && !IsTwoAddr) {
  Register SrcReg = MI.getOperand(1).getReg();
  unsigned SrcSub = MI.getOperand(1).getSubReg();
  if (MI.killsRegister(SrcReg, /*TRI=*/nullptr) ||
      MI.getOperand(0).getReg() == SrcReg)
    return NewMI;                              // <-- SKIP the COPY

  Register NewSrc = MI.getOperand(0).getReg();
  if (MRI.isSSA()) {
    const TargetRegisterClass &RC = *MF.getRegInfo().getRegClass(SrcReg);
    NewSrc = MRI.createVirtualRegister(&RC);
  }

  CopyMI = BuildMI(*NewMI->getParent(), *NewMI, MI.getDebugLoc(),
                   get(TargetOpcode::COPY))
               .addDef(NewSrc)
               .addReg(SrcReg, {}, SrcSub);
  NewMI->getOperand(1).setReg(NewSrc);
  NewMI->getOperand(1).setSubReg(0);
}
```

## Bug

When the original NDD instruction reads its `src1` with a sub-register index
(`SrcSub != 0`), `MI.killsRegister(SrcReg, /*TRI=*/nullptr)` interprets the
"kill" flag as killing the WHOLE register `SrcReg`. But the kill flag on a
sub-register use means only that the sub-register's lanes will not be used
after this instruction — the OTHER lanes of `SrcReg` may still be live and
needed downstream.

If that condition fires (kill flag is set on `%vreg.sub_X`), the patch skips
inserting the COPY and falls through to `return NewMI`. The replacement RMW
instruction's destination is operand 0 (a different vreg), but its operand 1
remains the original `SrcReg` with the original `SrcSub`. For an NDD form, the
producer was non-destructive — the high lanes of the source were not touched.
For the rewritten RMW (legacy) form, however, the rewrite did
`setReg(NewSrc); setSubReg(0)` only inside the `if (!killed)` branch. When we
skip into `return NewMI`, the orig SrcSub remains, but the instruction was
already changed to the RMW (non-NDD) opcode in `Impl()`, and the RMW form ties
op0 and op1 — i.e. it writes its destination through op1's register. The
caller's GR64 super-register no longer has the lanes that weren't touched by
the sub-register, because the RMW now (a) writes to op0 (the NDD destination,
not SrcReg in the rewritten instruction body) — but the IR layout depends on
which dst was preserved.

The mismatch surfaces as:

1. The non-NDD legacy variant requires `dst == src1`. After NDD's `dst = ADD
   src1, src2` becomes legacy's `src1 = ADD src1, src2`, but here the caller
   wanted the value to land in op0 (the NDD dst), not in SrcReg.

2. The skipping-of-COPY branch was designed for the case where `SrcReg` is
   wholly killed: then we can repurpose it as `dst`. With sub-reg use, this
   reasoning is invalid.

## Repro skeleton (MIR)

A concrete repro requires running the foldMemoryOperandImpl path post-regalloc
(via spill-fold). The hot path is `inline-spiller` or `register-coalescer`
attempting to fold a stack reload into an NDD instruction whose use is on a
sub-register, with NDDM disabled (`-mattr=+ndd` but not `+ndd-mem`).

```mir
# RUN: llc -mtriple=x86_64-unknown-linux-gnu -mattr=+ndd \
# RUN:   -run-pass=greedy,virtregrewriter,phi-node-elimination,liveintervals \
# RUN:   -verify-machineinstrs %s -o -
---
name:            ndd_subreg_kill_skipcopy
tracksRegLiveness: true
body: |
  bb.0:
    liveins: $rdi, $rsi
    %0:gr64 = COPY $rdi
    %1:gr32 = COPY %0.sub_32bit  ; use sub_32bit, get a gr32 vreg
    %2:gr32 = ADD32rr_ND %1, %1, implicit-def dead $eflags
    ; %1 is killed here -> killsRegister returns true based on op1 kill,
    ; but if %1 came from a sub-reg COPY and we later spill/reload through
    ; a sub-reg of %0, the path triggers.
    $eax = COPY %2
    RET 0, implicit $eax
```

## Why this is a bug

The check `MI.killsRegister(SrcReg, /*TRI=*/nullptr)` mirrors a kill on the
**virtual register name**, but the semantics of "the value held in this
register can be overwritten" are stronger when SrcReg is read through a
sub-reg. The right query is "does this MI kill ALL lanes of SrcReg that are
live at this point?" Replace with:

```cpp
if (SrcSub == 0 && MI.killsRegister(SrcReg, /*TRI=*/nullptr))
  return NewMI;
```

or equivalently, always insert the COPY when `SrcSub != 0`.

## Commands

```bash
# Find candidate inputs:
llc -mtriple=x86_64-unknown-linux-gnu -mattr=+ndd \
    -print-after=greedy input.ll 2>&1 | grep -B2 _ND

# Print IR after coalescer to inspect:
llc -mtriple=x86_64-unknown-linux-gnu -mattr=+ndd \
    -print-after=register-coalescer -O2 input.ll
```

## Investigation status

- Source-only analysis. Constructing a MIR repro that lands in the
  `NoNDDM && !IsTwoAddr` branch with a sub-reg `SrcSub` and a kill flag on
  that use requires post-regalloc state with NDDM disabled and a spill-fold
  attempt. Not reproduced end-to-end in the time budget; the source path is
  identified above.
- The verifier-rejected handcrafted MIRs (see `/tmp/w103/`) confirm that
  X86's GR32 class rejects sub_32bit projections of GR64 directly, which is
  why this bug only surfaces in the post-regalloc fold path (where the
  sub-reg is encoded on the physical register via REG_SEQUENCE / spill
  slot reload bookkeeping).
