; c013: `llvm.amdgcn.cube{id,ma,sc,tc}` mis-select on gfx940/gfx950
; (CDNA, no cube ALU).
;
; AMDGPU.td:1462 - FeatureGFX9 unconditionally includes FeatureCubeInsts.
; gfx940/941/942/950 inherit FeatureGFX9 via FeatureISAVersion9_4_Common
; (AMDGPU.td:1747) and implicitly carry FeatureCubeInsts even though
; CDNA chips lack the cube helper ALU.
;
; VOP3Instructions.td:264-269 - `SubtargetPredicate = HasCubeInsts`
; would gate it correctly if the feature were not over-claimed.
;
; Result: `llc -mcpu=gfx950 -O2` cleanly emits v_cubeid_f32 (opcode
; D1C4), v_cubema_f32 (D1C7), v_cubesc_f32 (D1C5), v_cubetc_f32 (D1C6).
; MC accepts; disasm confirms.  On gfx950 HW these would trap as
; illegal instructions.
;
; Sibling family: c001/c003/c004/c006/c008/c012 -- intrinsic without
; correct target gate.

source_filename = "c013-cube-intrinsics-wrong-gate-cdna"
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.amdgcn.cubeid(float, float, float)
declare float @llvm.amdgcn.cubema(float, float, float)
declare float @llvm.amdgcn.cubesc(float, float, float)
declare float @llvm.amdgcn.cubetc(float, float, float)

define amdgpu_kernel void @t(ptr addrspace(1) %p, float %a, float %b, float %c) {
  %i = call float @llvm.amdgcn.cubeid(float %a, float %b, float %c)
  %m = call float @llvm.amdgcn.cubema(float %a, float %b, float %c)
  %s = call float @llvm.amdgcn.cubesc(float %a, float %b, float %c)
  %t = call float @llvm.amdgcn.cubetc(float %a, float %b, float %c)
  %sum1 = fadd float %i, %m
  %sum2 = fadd float %s, %t
  %sum  = fadd float %sum1, %sum2
  store float %sum, ptr addrspace(1) %p
  ret void
}
