# m161: `SIInstrInfo::verifyInstruction` atomic vdst/vdata file-match check uses `isAGPR` (excludes AV-class)

*Discovery method: code inspection (AV-class audit; direct sibling family of m149/m152/m153).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIInstrInfo.cpp:5857-5869`
enforces that vdst/vdata of FLAT/MUBUF/DS atomics are both AGPR or
both VGPR on gfx90A+ via:

```cpp
if (RI.isAGPR(MRI, X) != RI.isAGPR(MRI, Y)) {
  ErrInfo = "Atomic instructions must be ...";
  return false;
}
```

`SIRegisterInfo::isAGPR` is defined as
`isAGPRClass(MRI.getRegClass(R))` which is `hasAGPRs(RC) &&
!hasVGPRs(RC)`.  AV_* classes have **both** VGPR and AGPR bits, so
`isAGPR` returns **false** for AV-class virtregs.

On gfx90A+ (gfx950 included), pre-RA `getLargestLegalSuperClass`
(`SIRegisterInfo.cpp:468`) intentionally widens VReg/AReg pairs to
AV_*.  So AV-class virtuals are the norm for many operands by the
time `verifyInstruction` runs.

## Two failure modes

1. **Spurious verifier reject**: AV-class vdst paired with true AGPR
   vdata:
   * `isAGPR(vdst) = false` (AV is not isAGPR)
   * `isAGPR(vdata) = true`
   * `false != true` -> verifier rejects valid IR.

2. **Silent acceptance of risky mismatch**: AV-class vdst paired with
   true VGPR vdata:
   * `isAGPR(vdst) = false` (AV is not isAGPR)
   * `isAGPR(vdata) = false`
   * `false == false` -> verifier passes silently, but final
     allocation may split vdst across A-half of the AV class while
     vdata stays VGPR.  The atomic encoding requires same-file
     vdst/vdata; allocation split corrupts the atomic.

## Reproducer

`reduced.ll` uses an MFMA result (typically AV-class after
`getLargestLegalSuperClass` promotion) as atomic vdata in a buffer
atomic.  The verifier and final allocator interact via the
isAGPR-blindness defect.

## Suggested fix

The check should treat AV as compatible with **both** VGPR and AGPR
register files.  Options:

* Replace `RI.isAGPR(X) != RI.isAGPR(Y)` with `RI.hasAGPRs(X) !=
  RI.hasAGPRs(Y) || RI.hasVGPRs(X) != RI.hasVGPRs(Y)` (correct
  but stricter than needed for AV).
* Predicate the verifier check on **final** (post-RA) register
  files; defer to later passes.
* Add explicit AV-class handling: `if (RI.isVectorSuperClass(X) ||
  RI.isVectorSuperClass(Y)) skip;` (since AV is settled at RA).

## Family

* m149 (SIPreAllocateWWMRegs uses `isVGPR` which excludes AV).
* m152 (getDestEquivalentVGPRClass strips AV-class).
* m153 (WholeWaveFunction prologue EXEC -- different but same WWM
  family).
* m161 (verifyInstruction atomic vdst/vdata uses isAGPR) -- this
  entry.

All four are sibling defects in the gfx90A+/gfx950 AV-class handling
family.  Root cause: helper predicates (`isAGPR`, `isVGPRClass`)
were designed for pre-AV-class architectures and don't account for
the unified AV super-class.

## Why the fuzzer hasn't caught it

* Most fuzz IR doesn't chain MFMA + atomic; the AV-class virtreg
  reach is narrow.  Per `MEMORY.md` (Prefer-random-over-idioms),
  the random emitter should chain `amdgcn.mfma.*` with `atomicrmw`
  / `cmpxchg` on the MFMA result.
* The defect manifests as a verifier diagnostic (mode 1) or a
  silent allocation split (mode 2).  Mode 2 is observable only via
  runtime testing of inactive-lane preservation.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Code path present; mode 1 reproduces with explicit `-verify-machineinstrs`. |
| ROCm 7.1.1 | Same defect. |
