# m152: `SIInstrInfo::getDestEquivalentVGPRClass` strips AV-class dest on gfx90A/gfx950

*Discovery method: code inspection (SIFixVGPRCopies/copy-fixup audit;
m149 sibling family).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIInstrInfo.cpp:9684-9688`
(`getDestEquivalentVGPRClass`, SrcRC-not-AGPR branch):

```cpp
} else {
  if (RI.isVGPRClass(NewDstRC) || NewDstRC == &AMDGPU::VReg_1RegClass)
    return nullptr;
  NewDstRC = RI.getEquivalentVGPRClass(NewDstRC);
```

`SIRegisterInfo::isVGPRClass` (`SIRegisterInfo.h:243`) is
`hasVGPRs(RC) && !hasAGPRs(RC)`.  AV_* classes have AGPR bits ->
return false.

On gfx90A+ (gfx950) `getLargestLegalSuperClass`
(`SIRegisterInfo.cpp:468-485`) promotes VReg_/AReg_ to AV_*, so
AV-class virtregs are the norm for COPY/PHI/REG_SEQUENCE/INSERT_SUBREG
dests.  In the `else` branch (SrcRC not AGPR), an AV-class dest is
**not early-returned** as already-OK; instead
`getEquivalentVGPRClass(AV_xx)` is invoked, silently demoting the
dest to a VGPR-only class.

## Effects

* `moveToVALU` rewrites the dest into a VGPR-only class, dropping
  AGPR legality.
* Subsequent MFMA / `V_ACCVGPR_*` uses see a class mismatch causing
  implicit truncation of the legal allocation set, extra
  `V_ACCVGPR_READ`/`WRITE` moves, or (if the source later becomes
  an AGPR) illegal-copy / verifier failures.

## Reproducer

`reduced.ll` chains `mfma.f32.16x16x4f32` (AV_512 def) -> divergent
`extractelement` (triggers `moveToVALU` on the AV-class def).
`getDestEquivalentVGPRClass` strips the AGPR legality.

## Suggested fix

Replace `isVGPRClass(NewDstRC)` with a check that also accepts AV
classes:

```cpp
if ((RI.hasVGPRs(NewDstRC) && !RI.isAGPRClass(NewDstRC)) ||
    NewDstRC == &AMDGPU::VReg_1RegClass)
  return nullptr;
```

Or only invoke `getEquivalentVGPRClass` when
`RI.isAGPRClass(NewDstRC)` is true.

## Family

* m149 (SIPreAllocateWWMRegs skips AV-class virtregs).
* m146 (resource AGPR undercount).

Same `isVGPRClass`-blindness root: any helper that uses
`isVGPRClass` to gate AV-aware logic mis-classifies AV virtregs.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely chains MFMA + divergent extract.  Per
  `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should chain `amdgcn.mfma.*` with divergent users.
* Manifests as silent class-mismatch / extra `V_ACCVGPR_*` moves
  -- visible in asm/MIR but not as O0/O2 value divergence.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | AV-class dest demoted to VGPR-only class. |
| ROCm 7.1.1 | Same defect. |
