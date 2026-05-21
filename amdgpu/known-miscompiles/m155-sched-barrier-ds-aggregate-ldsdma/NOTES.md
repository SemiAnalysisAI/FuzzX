# m155: `amdgcn.sched.barrier(0x800)` LDSDMA-allow still blocked by DS aggregate

*Discovery method: code inspection (amdgcn.sched.barrier 2nd-pass audit; m144 sibling).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUIGroupLP.cpp:2678-2685`
(`invertSchedBarrierMask`, DS clause):

```cpp
// VMEM clause (lines 2668-2676) -- correctly clears VMEM when LDSDMA allowed:
if ((InvertedMask & VMEM_READ) == NONE ||
    (InvertedMask & VMEM_WRITE) == NONE ||
    (InvertedMask & LDSDMA) == NONE)         // <-- LDSDMA-implies clause
  InvertedMask &= ~VMEM;

// DS clause (lines 2683-2685) -- MISSING LDSDMA-implies clause:
if ((InvertedMask & DS_READ) == NONE ||
    (InvertedMask & DS_WRITE) == NONE)
  InvertedMask &= ~DS;
```

When the user passes `mask = 0x800` (LDSDMA-allow), the inverter
clears VMEM/VMEM_READ/VMEM_WRITE via the symmetric VMEM clause but
leaves the **DS aggregate bit (0x80)** set.

`canAddMI` DS branch (`AMDGPUIGroupLP.cpp:2482-2484`):

```cpp
case SchedGroupMask::DS:
  Result = isDS(MI) || isLDSDMA(MI);    // <-- matches LDSDMA via DS bit
  break;
```

So the SchedGroup adds the LDSDMA op via the DS branch and pins it
behind the barrier, contradicting the documented semantics:

> AMDGPUUsage.rst:1626 -- "All LDSDMA instructions may be scheduled
> across sched_barrier" when bit 0x800 is set.

m144 fixed the VMEM-aggregate route to LDSDMA pinning; m155 covers
the symmetric DS-aggregate route.

## Reproducer

`reduced.ll` (identical structure to m144 but verifies the DS-side
defect):

```llvm
call void @llvm.amdgcn.global.load.lds(...)
call void @llvm.amdgcn.sched.barrier(i32 2048)
%x = load i32, ...
%y = add i32 %x, 1
store i32 %y, ...
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 -mllvm -print-after=amdgpu-igrouplp`
shows the LDSDMA op pinned to the barrier despite the user request.

Confirmed at the print layer: `sched.barrier.inverted.mask.ll:107-108`
asserts InvertedMask for input 2048 = 2031 = `0b011111101111` --
which still has DS (0x80) set.

## Suggested fix

Add the LDSDMA-implies-clear-DS clause symmetric to the VMEM clause:

```cpp
if ((InvertedMask & DS_READ) == NONE ||
    (InvertedMask & DS_WRITE) == NONE ||
    (InvertedMask & LDSDMA) == NONE)         // <-- ADD
  InvertedMask &= ~DS;
```

Or (subsumes m144 + m155): add an early-out in `canAddMI`: if
`isLDSDMA(MI) && (origMask & LDSDMA)` (user explicitly allowed),
skip both VMEM-aggregate and DS-aggregate classification branches
for this MI.

## Why the fuzzer hasn't caught it

* Same as m144: scheduling decisions aren't directly observable
  through the FuzzX O0/O2 oracle.  Need an asm-pattern oracle that
  inspects the MIR scheduling order against documented mask
  semantics.
* Per `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should generate `amdgcn.sched.barrier(mask)` with non-zero LDSDMA
  bit around real LDSDMA ops.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Bug present; LDSDMA pinned despite mask. |
| ROCm 7.1.1 | Same defect. |

## Family

* m144 (VMEM aggregate -- partial fix; the LDSDMA-allow path needs
  both the VMEM and DS routes blocked).
* m116 (sched.barrier mask encoding audit).
