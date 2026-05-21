# c013: `llvm.amdgcn.cube{id,ma,sc,tc}` mis-select on gfx940/gfx950 (CDNA, no cube ALU)

*Discovery method: code inspection (cube-intrinsic audit; sibling of c012).*

## The bug

`HasCubeInsts` is gated correctly on `V_CUBE{ID,SC,TC,MA}_F32`
patterns at `VOP3Instructions.td:264-269`.  BUT `FeatureGFX9`
itself **unconditionally includes** `FeatureCubeInsts` at
`AMDGPU.td:1462`.  Since gfx940/941/942/950 inherit `FeatureGFX9`
via `FeatureISAVersion9_4_Common` (`AMDGPU.td:1747`) and never
subtract `FeatureCubeInsts`, the predicate is true on CDNA-3
silicon that has no cube helper ALU.

Result: `llc -mcpu=gfx950 -O2` cleanly emits:

* `v_cubeid_f32` (opcode `D1C4`)
* `v_cubema_f32` (opcode `D1C7`)
* `v_cubesc_f32` (opcode `D1C5`)
* `v_cubetc_f32` (opcode `D1C6`)

MC accepts the encoding.  On real gfx950 HW these would trap as
illegal instructions.

## Reproducer

`reduced.ll`:

```llvm
declare float @llvm.amdgcn.cubeid(float, float, float)
declare float @llvm.amdgcn.cubema(float, float, float)
declare float @llvm.amdgcn.cubesc(float, float, float)
declare float @llvm.amdgcn.cubetc(float, float, float)

define amdgpu_kernel void @t(ptr addrspace(1) %p,
                             float %a, float %b, float %c) {
  %i = call float @llvm.amdgcn.cubeid(float %a, float %b, float %c)
  ...
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll`: cleanly emits
`v_cubeid_f32`/`v_cubema_f32`/`v_cubesc_f32`/`v_cubetc_f32`.

## Suggested fix

Option A: Drop `FeatureCubeInsts` from the `FeatureGFX9` base;
add it to `FeatureISAVersion9_0_Consumer_Common` and
`FeatureISAVersion9_0_MI_Common` explicitly (matching the c012
pattern: opt-in per chip, not blanket).

Option B: Subtract `FeatureCubeInsts` from
`FeatureISAVersion9_4_Common` via target-list arithmetic.

The intrinsic declarations in `IntrinsicsAMDGPU.td:557-575` should
also be marked unavailable on CDNA, or `LegalizerInfo` should
reject them at IR level for `!HasCubeInsts`.

## Adjacent NaN-handling defects (not separately filed)

* SDAG `isKnownNeverNaN` at `AMDGPUISelLowering.cpp:6359` lists
  only `amdgcn_cubeid`.
* SDAG `isCanonicalized` at `SIISelLowering.cpp:15670` lists only
  `amdgcn_cubeid`.
* GISel sibling at `SIISelLowering.cpp:15798-15801` lists **all
  four** (cubeid, cubema, cubesc, cubetc).

GISel over-promises: cubema returns `2*max(|x|,|y|,|z|)` which
propagates NaN; cubesc/cubetc select face-coords and propagate
NaN.  Also, cubeid's unconditional `return true` in SDAG ignores
NaN-input case.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `amdgcn.cube*` intrinsics on
  compute-only CDNA targets.  Per `MEMORY.md`
  (Prefer-random-over-idioms), the random emitter should add
  `amdgcn.cube{id,ma,sc,tc}` to the pool for ALL targets and
  expect either a clean diagnostic or correct codegen.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Emits invalid `v_cube*_f32` instructions. |
| ROCm 7.1.1 | Same defect. |

## Family

* c001 (sudot), c003 (permlane16), c004 (dpp8), c005 (global.load.lds),
  c006 (tanh.f16), c008 (class.bf16), c012 (pops.exiting.wave.id) --
  same family of "intrinsic without correct target gate on CDNA".
