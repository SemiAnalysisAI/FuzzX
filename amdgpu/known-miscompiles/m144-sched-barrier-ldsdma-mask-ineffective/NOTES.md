# m144: `llvm.amdgcn.sched.barrier` LDSDMA mask bit (0x800) is silently ineffective

*Discovery method: code inspection (during amdgcn.iglp.opt / sched.* audit).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUIGroupLP.cpp:2667-2676`
(`invertSchedBarrierMask`):

```cpp
if (Mask & SchedBarrierMasks::LDSDMA) {
  InvertedMask &= ~static_cast<MaskType>(SchedBarrierMasks::VMEM);
  // ...
}
```

The inverter clears the aggregate `VMEM` bit but leaves the
**VMEM_READ** (0x10) and **VMEM_WRITE** (0x20) sub-bits set.

`SIInstrInfo::isLDSDMA` (`SIInstrInfo.h:631`):

```cpp
bool isLDSDMA(const MachineInstr &MI) const {
  return (isVALU(MI) && (isMUBUF(MI) || isFLAT(MI))) ||
         isTENSOR_CNT(MI);
}
```

Every LDSDMA instruction therefore also satisfies
`isVMEM(MI) && mayLoad/mayStore`.

`canAddMI` (`AMDGPUIGroupLP.cpp:2474-2480`) classifies any LDSDMA
instruction into the SchedGroup via the VMEM_READ / VMEM_WRITE
branches.  The instruction receives ordering edges to the
SCHED_BARRIER and **cannot move past**, contradicting the documented
semantics in `AMDGPUUsage.rst:1626`:

> All LDSDMA instructions may be scheduled across sched_barrier
> [when bit 0x800 is set].

### Asymmetric mask behaviour

* Requesting **DS-allow** (mask = 0x80) correctly allows LDSDMA --
  line 2680 clears the LDSDMA bit when DS is allowed.
* Requesting **LDSDMA-allow** (mask = 0x800) fails to allow LDSDMA
  itself, because the VMEM_READ/WRITE sub-bits still classify the
  LDSDMA op.

## Reproducer

`reduced.ll` -- one `amdgcn.global.load.lds` (an LDSDMA op) followed
by `amdgcn.sched.barrier(2048)` and independent VALU work.  The
documented semantics say the LDSDMA may schedule across the barrier;
in practice the scheduler keeps it pinned.

Build:

```
llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll
```

Inspect the emitted MIR with `-mllvm -print-after=amdgpu-igrouplp`
-- the LDSDMA op stays before the barrier despite the mask requesting
allow-past.

## Existing test gap

`test/CodeGen/AMDGPU/sched.barrier.inverted.mask.ll:107-114` exists
and verifies the debug-print mask value 2031 (`0b011111101111`)
when input is 2048.  But the test only checks the print; it never
runs an actual LDSDMA op through the barrier to verify the
scheduling effect.  The bug is not visible at the mask-print layer.

## Suggested fix

In `invertSchedBarrierMask`, when clearing the LDSDMA bit, also clear
the VMEM_READ and VMEM_WRITE sub-bits, OR add an early-out in
`canAddMI`: if `isLDSDMA(MI) && (SGMask & LDSDMA) == 0` (LDSDMA was
explicitly allowed by the user), skip the VMEM_READ/WRITE classifier
branches for this MI.

Concretely:

```cpp
if (Mask & SchedBarrierMasks::LDSDMA) {
  InvertedMask &= ~static_cast<MaskType>(SchedBarrierMasks::VMEM);
  InvertedMask &= ~static_cast<MaskType>(SchedBarrierMasks::VMEM_READ);   // ADD
  InvertedMask &= ~static_cast<MaskType>(SchedBarrierMasks::VMEM_WRITE);  // ADD
}
```

## Why the fuzzer hasn't caught it

* The existing oracle compares scalar/value outputs O0 vs O2.
  Scheduling decisions are not directly observable through this
  oracle unless they cause an actual data race or change a
  latency-visible metric.
* Per `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should generate `amdgcn.sched.barrier(mask)` with non-zero LDSDMA
  bit set around real LDSDMA ops, and the oracle should compare
  MIR scheduling order against the documented mask semantics.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Bug present; scheduler keeps LDSDMA pinned despite mask. |
| ROCm 7.1.1 | Same defect. |

## Family

* m116 (sched.barrier mask audit): identified mask-encoding subtleties.
  m144 extends that finding to a concrete LDSDMA-bit miscompile.
* AMDGPUUsage.rst:1626 is the documented contract; the in-code
  implementation deviates.
