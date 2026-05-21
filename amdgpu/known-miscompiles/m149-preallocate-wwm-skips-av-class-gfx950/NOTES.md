# m149: `SIPreAllocateWWMRegs` skips AV-class virtregs on gfx90A/gfx950 -> WWM inactive-lane corruption

*Discovery method: code inspection (during SIPreAllocateWWMRegs audit).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIPreAllocateWWMRegs.cpp:102`
(`processDef`):

```cpp
if (!TRI->isVGPR(*MRI, Reg))
    return false;
```

`SIRegisterInfo::isVGPR` (`SIRegisterInfo.cpp:3824`) calls
`isVGPRClass` (`SIRegisterInfo.h:243-245`):

```cpp
bool isVGPRClass(const TargetRegisterClass *RC) const {
  return hasVGPRs(RC) && !hasAGPRs(RC);
}
```

On gfx90A+ (gfx950 included) MAI-capable register classes use the
unified vector super-class (AV_32 / AV_64 / AV_512 / etc.; see
`SIRegisterInfo.cpp:3645-3651` `getCompatibleSubRegClass` and
`isVectorSuperClass` at `SIRegisterInfo.h:253-255`).  AV classes have
both VGPR and AGPR bits, so `hasAGPRs(RC) == true`, and the early
return at line 102 fires.

The WWM pre-allocator silently skips the virtreg.

## Effects

1. The virtreg is left to the per-thread VGPRAllocator, which is
   unaware of WWM semantics.
2. The physreg is never added to `WWMReservedRegs`, so VGPRAllocator
   may reuse the physreg across `EXIT_STRICT_WWM` for another live
   virtreg.
3. Post-EXIT writes execute under restored EXEC and overwrite only
   active lanes, corrupting inactive-lane data the WWM live range
   was supposed to preserve.

## Companion defect

`SILowerWWMCopies.cpp:135` (`addToWWMSpills`) inherits the same
`isVGPRClass`-only assumption, missing the prolog/epilog spill
insertion for AV-class WWM virtregs on gfx90A/gfx950.

## Reproducer

`reduced.ll` wraps an MFMA result (AV_512) in `llvm.amdgcn.strict.wwm`.
The MFMA destination is an AV-class virtreg.  `SIPreAllocateWWMRegs`
silently skips it; downstream `fadd` reuses VGPRs and overwrites
inactive lanes of the supposedly-WWM live value.

```llvm
%mf  = call <16 x float> @llvm.amdgcn.mfma.f32.16x16x4f32(...)  ; AV_512 def
%wwm = call <16 x float> @llvm.amdgcn.strict.wwm.v16f32(<16 x float> %mf)
%sum = fadd <16 x float> %wwm, %z       ; reuse of wwm's physreg under
                                        ; restored EXEC corrupts inactive lanes
```

Test with active-lane mask < wave mask (e.g. masked dispatch / use
`llvm.amdgcn.set.inactive` to seed inactive lanes with known
constants).  Inactive-lane corruption is observable on output.

## Suggested fix

Replace the `TRI->isVGPR(...)` early-exit with a more permissive
check that also accepts AV-class virtregs:

```cpp
const TargetRegisterClass *RC = MRI->getRegClass(Reg);
if (!TRI->hasVGPRs(RC))    // accepts both VGPR-only and AV classes
  return false;
```

Or equivalently use `TRI->isVectorRegister(...)` (also true for AV
classes).  Same fix in `SILowerWWMCopies.cpp:135`.

On gfx90A+, the WWM pre-allocator must treat AV classes as eligible
WWM destinations and reserve the selected physreg in
`WWMReservedRegs` so the per-thread allocator does not reuse it
across EXIT_STRICT_WWM.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely generates `llvm.amdgcn.strict.wwm` /
  `llvm.amdgcn.set.inactive` on MFMA outputs.  Per `MEMORY.md`
  (Prefer-random-over-idioms), the random emitter should:
  - emit `amdgcn.mfma.*` -> `amdgcn.strict.wwm` chains
  - dispatch with masked active-lane sets
  - compare inactive-lane outputs against the seeded inactive
    values
* The differential O0-vs-O2 oracle in active-lane-only mode would
  miss this -- needs an inactive-lane-aware oracle.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | AV-class virtreg silently skipped by `processDef`. |
| ROCm 7.1.1 | Same defect. |

## Family

* m099/m131/m086 (`set_inactive` divergent witness / KnownBits over-promise).
* m119 (target-side ordering / cache-control).
* WWM/AGPR family on gfx950 unified-vector classes is generally
  fragile; this is the first miscompile-class entry for it.
