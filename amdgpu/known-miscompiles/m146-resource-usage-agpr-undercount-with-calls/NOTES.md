# m146: `AMDGPUResourceUsageAnalysis` asymmetric AGPR/VGPR tracking in call-path branch

*Discovery method: code inspection (during AMDGPUResourceUsageAnalysis audit).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUResourceUsageAnalysis.cpp:173-177`
(fast-path init) and lines 188-321 (per-MI call-path scan).

In `analyzeResourceUsage`, when `FrameInfo.hasCalls()` is true:

* **`NumVGPR` is recomputed** by the per-MI MaxVGPR scan at lines
  240-253, ultimately stored at line 319.
* **`NumAGPR` is set ONCE** at line 176 via
  `getNumUsedPhysRegs(AGPR_32RegClass, IncludeCalls=false)` and
  **never updated by the scan** (line 244 filters to
  `isVGPRClass(RC)` only, falling through to `continue` for
  AGPR/SGPR classes).
* **`NumExplicitSGPR` is also set ONCE** at line 173 with
  `IncludeCalls=false`; the call-path scan never updates it
  either.

```cpp
if (ST.hasMAIInsts())
  Info.NumAGPR = TRI.getNumUsedPhysRegs(AMDGPU::AGPR_32RegClass,
                                        /*IncludeCalls=*/false);
```

Because `IncludeCalls=false` forces `SkipRegMaskTest=true`, SGPRs
def'd across a call (visible only via the call's regmask) and AGPRs
written by inlined callees are not counted.  The call-path scan then
neglects to update these counters.

## Consequence on gfx950

* MAI/AGPR-heavy kernels with calls under-report AGPR usage.
* Combined with **unified VGPR+AGPR allocation** on gfx90a/gfx950
  (registers must be balanced/aligned for occupancy computation),
  downstream `max(NumVGPR, NumAGPR)` for the kernel descriptor's
  `.agpr_count` field may select the stale low AGPR count.
* The runtime / kernel-descriptor consumer then dispatches with a
  higher waves-per-CU than the actual peak AGPR usage permits ->
  register spill at runtime or wave underutilization.

## Secondary defect

The call-path MaxVGPR scan at lines 188-253 lacks the
`getAddressableNumArchVGPRs()` clip that the fast-path applies
(`SIRegisterInfo.cpp:4121-4125`).  On wave32 (gfx950 supports both
wave32 and wave64) there are fewer addressable archVGPRs than
wave64; the scan can compute `MaxVGPR+1 > addressable` and nothing
here clamps or reports an error.

## Reproducer

`reduced.ll` calls an external function between two MFMA operations
that write `<16 x float>` AGPR ranges.  The pre-call MaxVGPR scan
sees no AGPR writes inside the kernel itself; the post-call NumAGPR
snapshot was taken once with `IncludeCalls=false` and never updated.
Reported `.agpr_count` in the kernel descriptor is lower than the
actual peak AGPR usage when the callee uses AGPRs.

Inspect with:

```
llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll -o reduced.s
grep agpr_count reduced.s
```

Compare against an inlined-callee variant -- the inlined variant
correctly counts AGPR usage; the called variant under-reports.

## Suggested fix

In the call-path loop (188-253), also update `MaxAGPR` and
`MaxSGPR` counters when `RC` is AGPR/SGPR class, mirroring the
`MaxVGPR` logic.  Set `Info.NumAGPR` and `Info.NumExplicitSGPR`
to `max(initial, MaxX+1)`.  Apply `getAddressableNumArchVGPRs()`
clamp post-scan.

Concretely, replace the `!TRI.isVGPRClass(RC)` continue at line 244
with separate branches for VGPR / AGPR / SGPR class accumulation.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits MFMA + external-call patterns.  Per
  `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should generate `amdgcn.mfma.*` intrinsics interleaved with
  external function calls.
* The differential O0-vs-O2 oracle compares stored values, not
  kernel-descriptor metadata.  An asm-pattern oracle that compares
  reported `.agpr_count` against the actual peak AGPR usage would
  catch this.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | AGPR count stays at pre-call value despite post-call AGPR use. |
| ROCm 7.1.1 | Same defect. |
