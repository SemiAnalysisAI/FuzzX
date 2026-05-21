# c012: `llvm.amdgcn.pops.exiting.wave.id` selects on gfx940/gfx950 (CDNA, no POPS HW)

*Discovery method: code inspection (during amdgcn.exp/pops intrinsic audit).*

Sibling shape to c001/c003/c004/c005/c006/c008 -- intrinsic without
correct target gate.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SOPInstructions.td:2050-2054`:

```tablegen
let SubtargetPredicate = isGFX9GFX10 in {
def : GCNPat <
  (i32 (int_amdgcn_pops_exiting_wave_id)),
  (S_MOV_B32 (i32 SRC_POPS_EXITING_WAVE_ID))
>;
}
```

The `isGFX9GFX10` predicate is true for **gfx940/gfx942/gfx950**
(Generation = GFX9).  But POPS (Primitive Ordered Pixel Shading) is
a graphics-pipe-only HW feature absent on the CDNA gfx940/gfx950
line.  `SRC_POPS_EXITING_WAVE_ID` is not a valid SGPR source on
those targets.

Result: the intrinsic selects to:

```asm
s_mov_b32  s0, src_pops_exiting_wave_id
```

which the MC layer either rejects (assembler error) or accepts as a
binary that triggers an illegal-instruction trap at runtime on
gfx950 HW.

## Reproducer

`reduced.ll`:

```llvm
declare i32 @llvm.amdgcn.pops.exiting.wave.id()

define amdgpu_kernel void @t(ptr addrspace(1) %p) {
  %r = call i32 @llvm.amdgcn.pops.exiting.wave.id()
  store i32 %r, ptr addrspace(1) %p
  ret void
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll`: emits an invalid
`s_mov_b32 src_pops_exiting_wave_id` reference.

## Suggested fix

Predicate the pattern with `isGFX9GFX10 && !hasGFX940Insts()` or
introduce a dedicated `HasPOPS` subtarget feature gated on
graphics-only generations.

The intrinsic itself (`IntrinsicsAMDGPU.td`) could also be marked
unavailable for CDNA targets via the existing target-availability
mechanism.

## Adjacent defect

The same audit found that `SIISelLowering.cpp:12024`
(`amdgcn.exp.compr` lowering guard) uses `hasCompressedExport()` =
`!HasGFX11Insts`.  On gfx950 this returns *true* (gfx950 has no
GFX11), but gfx950 also has no export HW at all
(`hasExportInsts() = !hasGFX940Insts() = false`).  No diagnostic
fires; the code then emits a target-machine `AMDGPU::EXP`/`EXP_DONE`
node via `getMachineNode`, bypassing the `SubtargetPredicate =
HasExportInsts` gate on the pseudo (`EXPInstructions.td:61`).

Not filed separately -- same family as this entry; both are guarded
by predicates that don't match the actual HW capability.  If
multiple distinct repros surface, file as c013.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `amdgcn.pops.*` intrinsics.  Per
  `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should add `amdgcn.pops.exiting.wave.id` and `amdgcn.exp.*` to
  the intrinsic pool for compute targets, expecting either a clean
  diagnostic or correct codegen.
* The MC assembler may accept the invalid SGPR source on some
  toolchain configurations, silently producing an
  unencodable-on-this-HW binary.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Pattern fires; emits invalid SGPR source. |
| ROCm 7.1.1 | Same defect. |
