# w103: TwoAddressInstructionPass::processTiedPairs adds LIS subrange segments for ALL lanes regardless of source sub-reg

## Source

`amdgpu/third_party/llvm-project/llvm/lib/CodeGen/TwoAddressInstructionPass.cpp`,
function `TwoAddressInstructionImpl::processTiedPairs`, lines 1670-1692.

```cpp
if (LIS) {
  LastCopyIdx = LIS->InsertMachineInstrInMaps(*PrevMI).getRegSlot();

  SlotIndex endIdx =
      LIS->getInstructionIndex(*MI).getRegSlot(IsEarlyClobber);
  if (RegA.isVirtual()) {
    LiveInterval &LI = LIS->getInterval(RegA);
    VNInfo *VNI = LI.getNextValue(LastCopyIdx, LIS->getVNInfoAllocator());
    LI.addSegment(LiveRange::Segment(LastCopyIdx, endIdx, VNI));
    for (auto &S : LI.subranges()) {          // <-- iterates ALL subranges
      VNI = S.getNextValue(LastCopyIdx, LIS->getVNInfoAllocator());
      S.addSegment(LiveRange::Segment(LastCopyIdx, endIdx, VNI));
    }
  } else {
    ...
  }
}
```

## Bug

The COPY inserted just before MI is:
```
NewCopy: RegA = COPY RegB:SubRegB
```

When `SubRegB != 0`, this COPY defines only the lanes implied by SubRegB, not
ALL lanes of RegA. But the loop above unconditionally creates a value number
and adds a segment `[LastCopyIdx, endIdx)` to EVERY subrange of LI (the live
interval for RegA), regardless of whether SubRegB covers that subrange.

For RegA on a target that tracks sub-reg liveness (AArch64, AMDGPU, ARM,
others), this produces an inconsistent LiveInterval: subranges for lanes that
were NOT defined by the COPY have a freshly-allocated VNInfo at LastCopyIdx,
which the verifier interprets as a redefinition.

X86 does not currently enable sub-reg liveness, so this is currently latent
on X86, but the bug lives in target-independent CodeGen and fires on any
backend that does enable it.

## Repro

```mir
# RUN: llc -mtriple=aarch64-linux-gnu -run-pass=liveintervals,twoaddressinstruction \
# RUN:   -verify-machineinstrs %s -o -
---
name:            two_addr_subreg_lis
tracksRegLiveness: true
body: |
  bb.0:
    liveins: $q0
    %0:fpr128 = COPY $q0
    ; Two-addr instruction where src is a sub-reg of a wider vreg.
    ; A truncation COPY must be inserted; the dst's subranges must be
    ; updated only for the lanes defined by sub_dsub.
    %1:fpr64 = INSvi64lane undef %1, 0, %0, 0
    $d0 = COPY %1
    RET_ReallyLR implicit $d0
```

## Why this is a bug

The fix is to compute the lane mask for SubRegB, then only add segments to
subranges whose `LaneMask` intersects:

```cpp
LaneBitmask CopiedLanes = SubRegB
    ? TRI->getSubRegIndexLaneMask(SubRegB)
    : MRI->getMaxLaneMaskForVReg(RegA);
for (auto &S : LI.subranges()) {
  if ((S.LaneMask & CopiedLanes).none()) continue;
  VNI = S.getNextValue(LastCopyIdx, LIS->getVNInfoAllocator());
  S.addSegment(LiveRange::Segment(LastCopyIdx, endIdx, VNI));
}
```

If RegA is fully-defined by the COPY (i.e., the COPY is itself widening RegA
or RegA's class has no subranges that are missing from the COPY), the
original behavior is correct. With a truncating COPY (subreg src), it's not.

## Investigation status

- Source analysis: TwoAddressInstructionPass.cpp:1679-1682 unconditionally
  iterates ALL subranges of LI and adds segments. The comment block
  immediately above (at line 1651-1661) explicitly acknowledges
  "tied subregister must be a truncation" — i.e., the pass KNOWS the COPY
  is truncating, but does not propagate that to the subrange update.
- X86 does not enable sub-reg liveness (see X86RegisterInfo), so the issue
  is latent in X86 builds. Repro needs a target with sub-reg liveness +
  sub-reg use on tied operand source.
- AArch64 / AMDGPU / ARM are candidate targets; AArch64 is most likely to
  hit this in normal code because FPR128 vs FPR64 sub-reg use is common.
