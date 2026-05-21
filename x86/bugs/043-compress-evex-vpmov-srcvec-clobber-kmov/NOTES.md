## X86CompressEVEX: tryCompressVPMOVPattern allows SrcVec clobber between MI and final mask read

**File:** `llvm/lib/Target/X86/X86CompressEVEX.cpp:270-318`

### Reasoning

After the VPMOV*2M -> VMOVMSK rewrite, the new `VMOVMSK` instruction is
**placed at KMovMI's position**, replacing the KMOV in place:

```cpp
KMovMI->setDesc(TII->get(MovMskOpc));
KMovMI->getOperand(1).setReg(SrcVecReg);
```

For this to be semantically equivalent, `SrcVecReg` must have the same
value at both the original VPMOV*2M position (where the mask was computed)
and at KMovMI's position (where the new MOVMSK reads it).

The forward scan only protects this when KMovMI is **not yet found**:

```cpp
if (!KMovMI && CurMI.modifiesRegister(SrcVecReg, TRI)) {
  return false;
}
```

So the order of checks per instruction inside the loop matters. Reading
the code:

```cpp
for (... CurMI ...) {
  if (CurMI.readsRegister(MaskReg, TRI)) {
    if (KMovMI) return false;     // multiple uses
    if (IsKMOV && ...) {
      KMovMI = &CurMI;
      // continue scanning
    } else {
      return false;
    }
  }
  if (CurMI.modifiesRegister(MaskReg, TRI)) {
    if (!KMovMI) return false;
    break;                         // <-- exits loop AT mask redef
  }
  if (!KMovMI && CurMI.modifiesRegister(SrcVecReg, TRI)) {
    return false;
  }
}
```

There is no symmetric `modifiesRegister(SrcVecReg)` check *after* KMovMI
is found, but that's fine because the new VMOVMSK is *at* KMovMI's slot
— anything modifying SrcVecReg *after* KMovMI does not affect what
KMovMI reads.

So the bug is not here. But there is a subtle one nearby:

**Real bug**: when the KMOV reads the mask and *also* modifies SrcVecReg
in the same instruction, the check
`if (!KMovMI && CurMI.modifiesRegister(SrcVecReg, TRI))` runs **after**
the read-check has already set `KMovMI = &CurMI`. So a KMOV that, by
construction, would clobber the SrcVec (none of the KMOVrk variants do
this in practice — they only read a k-reg and write a GPR) would slip
through. Today KMOVBrk/KMOVWrk/KMOVDrk all have signature `(GR, K)` and
do not touch the XMM source, so this is theoretical.

### What's actually wrong

The bigger issue: this pass does not validate that `SrcVecReg` is the
**same physical register class** that VMOVMSK expects. `VMOVMSKPSrr`
takes `VR128` (XMM0-15 for VEX). `VPMOVQ2MZ128kr` takes `VR128X` (XMM0-31).
The pass does check `usesExtendedRegister(MI)` to ensure no XMM16-31 are
used (line 240). Good.

However, the rewrite also does:

```cpp
KMovMI->getOperand(1).setReg(SrcVecReg);
```

without updating any kill flags on SrcVecReg. If the original VPMOV*2M
had a kill flag on SrcVecReg, that flag is **on MI**, which is about to
be erased. The new VMOVMSK at KMovMI carries whatever kill flag KMovMI's
operand 1 (the k-register) had — typically `kill $k0`, which is then
overwritten with `$xmm0` losing whether it should be a kill. If
SrcVecReg is live past KMovMI in the same MBB or successor, this is
fine. If SrcVecReg was killed at MI (no later uses), the new MOVMSK
should be the kill point — but the rewritten operand may not have its
kill flag set. Liveness then says SrcVecReg has a missing kill.

This produces stale liveness info, which can confuse later passes
(BranchFolder, RegisterCoalescing should be done by now but post-RA
scheduler runs after). Worst case: a stale liveness can make a later
pass keep the register live longer than needed (perf) or — if a pass
uses the kill flag to drive a transformation — produce wrong code.

### Severity

Liveness staleness; usually not a miscompile in practice but a real
post-RA invariant violation.
