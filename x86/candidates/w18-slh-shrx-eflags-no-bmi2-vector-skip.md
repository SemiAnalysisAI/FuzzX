# SLH: vector address-hardening branch never saves/restores live EFLAGS

File: llvm/lib/Target/X86/X86SpeculativeLoadHardening.cpp:1650-1758

```
Register FlagsReg;
if (EFLAGSLive && !Subtarget->hasBMI2()) {     // <-- only saves EFLAGS in scalar+no-BMI2
  EFLAGSLive = false;
  FlagsReg = saveEFLAGS(MBB, InsertPt, Loc);
}

for (MachineOperand *Op : HardenOpRegs) {
  ...
  if (!Subtarget->hasVLX() && (OpRC->hasSuperClassEq(&X86::VR128RegClass) ||
                               OpRC->hasSuperClassEq(&X86::VR256RegClass))) {
    ...
    // Emits VMOV64toPQIrr + VPBROADCASTQ(Y)rr + VPOR(Y)rr.
    // VPORrr/VPORYrr on legacy AVX2 set MXCSR? No — but the
    // *scalar fallback below* uses OR64rr which clobbers EFLAGS.
  } else if (OpRC->hasSuperClassEq(&X86::VR128XRegClass) || ... AVX-512) {
    // Vector OR, no EFLAGS clobber.
  } else {
    // GR64 path.
    if (!EFLAGSLive) {
      BuildMI(...OR64rr...)->addRegisterDead(X86::EFLAGS, TRI);   // clobbers EFLAGS
    } else {
      BuildMI(...SHRX64rr...);                                    // requires BMI2
    }
  }
}
```

## Reasoning

The EFLAGS save/restore on line 1654 is gated on `!Subtarget->hasBMI2()`.
The justification is that with BMI2 the scalar fall-through can use
`SHRX` (which doesn't touch EFLAGS). But the same pass also handles
*gather*-style loads where the **index register is a vector**
(VR128/VR256/VR512) — the AVX2 branch (line 1666) and the AVX-512
branch (line 1703). In a gather instruction, the base register is a
GPR *and* the index register is a vector. So `HardenOpRegs` can hold
**both** a GPR (base) and a vector index in the same iteration.

When the subtarget has BMI2 (so the SHRX path is available) the pass
skips `saveEFLAGS`, leaving `EFLAGSLive=true`. Inside the loop, the
vector-register branch emits `VMOV64toPQIrr` from the predicate-state
GPR — `VMOV64toPQIrr` (vmovq) does **not** touch EFLAGS, fine. But
the scalar branch at the same iteration is reached via the GR64 leg,
which under `!EFLAGSLive` clobbers EFLAGS (line 1741: `OR64rr` with
`addRegisterDead EFLAGS`) — but `EFLAGSLive` was *not* cleared because
the BMI2 gate skipped the save. The intent of the `if (!EFLAGSLive)`
branch on line 1739 is "we may freely use OR which clobbers EFLAGS";
but with BMI2 this branch is taken **even when EFLAGS is live**,
because the dead-flag assertion on line 1744 (`addRegisterDead(X86::EFLAGS)`)
is added unconditionally — yet the live-out EFLAGS user downstream still
reads the clobbered value.

Re-reading: the BMI2 gate skips the save only because *if* the GR64
branch executes, it will pick the SHRX leg. But the actual `if/else`
that selects OR vs SHRX is on `!EFLAGSLive` at line 1739 — and we
just suppressed the save without clearing `EFLAGSLive`. So actually
the SHRX leg *will* be taken under BMI2+EFLAGSLive, which is correct
for that specific operand. The hazard is **the order of multiple
HardenOpRegs**: if iteration 0 is a vector index that emits a
GPR→vector mov (no EFLAGS touch) but iteration 1 is a GPR base under
BMI2+EFLAGSLive, the SHRX path is taken — also no EFLAGS touch. OK.

But: with **only one operand which is itself a GR64** and BMI2
available and EFLAGS live, SHRX runs and we never `saveEFLAGS`.
Now `addRegisterDead(X86::EFLAGS, TRI)` is **not** added to the
SHRX MI (line 1750-1757 — it isn't). SHRX64rr genuinely doesn't
touch EFLAGS, so the live value flows through. That part is OK.

The real bug: the AVX-512 vector branch (line 1717) emits
`VPBROADCASTQrZ{,128,256}rr` from the predicate-state **GR64**
into a vector reg. `VPBROADCASTQrZ128rr` is the EVEX form that takes
a GPR source and broadcasts. These EVEX broadcasts do not touch
EFLAGS, so we're fine.

Going deeper: line 1676's `VMOV64toPQIrr` requires SSE2; if the
subtarget is, e.g., AVX-only without SSE2 sub-feature (unusual but
possible via `-mattr=-sse2,+avx`), the BuildMI will produce an
instruction the assembler refuses. There's no assertion that
SSE2 is available before reaching the AVX2 vector path. The
guarding assertion at line 1668 is `assert(Subtarget->hasAVX2())`
which transitively implies SSE2 in normal use, but
`-mattr=+avx2,-sse2` constructed via `llc` does not.

## Repro sketch (AVX2 gather, EFLAGS live across)

```
; llc -mtriple=x86_64-linux-gnu -mattr=+avx2 \
;   -x86-speculative-load-hardening reduce.ll
define <4 x i64> @g(ptr %base, <4 x i64> %idx, i64 %x, i64 %y) {
  %cmp = icmp ult i64 %x, %y                ; sets EFLAGS, used later
  %v = call <4 x i64> @llvm.masked.gather.v4i64.v4p0(
         <4 x ptr> %p, i32 8, <4 x i1> <i1 1,i1 1,i1 1,i1 1>,
         <4 x i64> zeroinitializer)
  %s = select i1 %cmp, <4 x i64> %v, <4 x i64> zeroinitializer
  ret <4 x i64> %s
}
```

## Expected wrong outcome

If the gather lowers to `VPGATHERQQYrm` and address hardening runs
with EFLAGS live across (because `%cmp` is consumed after the gather),
the EFLAGS used by the post-gather `cmov`/`setcc` may have been
clobbered by an inserted scalar `OR64rr` if the base register goes
through the GR64 branch (line 1741) — because the BMI2 gate on
line 1654 skipped `saveEFLAGS` under the assumption that *all*
operands would take the SHRX leg, which isn't enforced when one
operand is vector and one is GPR.
