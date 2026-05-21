; c012: `llvm.amdgcn.pops.exiting.wave.id` lowers to invalid
; SRC_POPS_EXITING_WAVE_ID SGPR on gfx940/gfx950.
;
; SOPInstructions.td:2050-2054 gates the selection pattern with
; `isGFX9GFX10`, which is true for gfx940/gfx950 (Generation = GFX9).
; But POPS (Primitive Ordered Pixel Shading) is a graphics-only HW
; feature absent on the CDNA gfx940/gfx950 line.
;
; Result: the intrinsic selects to `S_MOV_B32 r, src_pops_exiting_wave_id`,
; which is not a valid SGPR source on gfx940/gfx950.  The MC layer
; either rejects the encoding or produces a binary that triggers an
; illegal-instruction trap at runtime.
;
; Fix: predicate the pattern with `isGFX9GFX10 && !hasGFX940Insts()`
; or introduce a dedicated `HasPOPS` subtarget feature.
;
; Sibling to c001 (sudot - wrong target gate), c003 (permlane16),
; c004 (dpp8), c005 (global.load.lds), c006 (tanh.f16/.f32 bf16),
; c008 (class.bf16), c009 (ballot wrong width), c010 (strict bf16
; extend), c011 (TFE+illegal data type).

source_filename = "c012-pops-exiting-wave-id-wrong-gate-cdna"
target triple = "amdgcn-amd-amdhsa"

declare i32 @llvm.amdgcn.pops.exiting.wave.id()

define amdgpu_kernel void @t(ptr addrspace(1) %p) {
  %r = call i32 @llvm.amdgcn.pops.exiting.wave.id()
  store i32 %r, ptr addrspace(1) %p
  ret void
}
