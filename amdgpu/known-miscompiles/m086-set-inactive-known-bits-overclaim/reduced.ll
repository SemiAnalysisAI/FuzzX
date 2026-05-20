; RUN-INPUTS: [0xCAFEBABE]
target triple = "amdgcn-amd-amdhsa"

; Bug: AMDGPUTargetLowering::SimplifyDemandedBitsForTargetNode for
; amdgcn.set_inactive only recurses into operand(1) (the "value") and
; ignores operand(2) (the "inactive_value"). When 'value' is a
; constant, the generic SimplifyDemandedBits framework folds the
; entire intrinsic to the value constant -- silently dropping the
; inactive_value contribution.
;
; The bug is directly visible in the generated assembly at -O2:
; the set_inactive node is replaced by a constant `s_mov_b32 s4,
; 0xaaaa`, with no `v_cndmask` between value and inactive_value,
; whereas at -O0 the proper cndmask is emitted.
;
; The current harness launches block_dim=256, so every wave runs with
; EXEC=all-ones; partial waves can only be created by divergent
; control flow, which then forces a PHI that overwrites the
; inactive_value contribution anyway. The bug therefore manifests
; only in the IR/asm under -O2 vs -O0 -- not in the harness's
; round-trip runtime comparison. See NOTES.md.

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %is0 = icmp eq i32 %wi, 0
  br i1 %is0, label %do_si, label %ret

do_si:
  ; Lane 0 enters under a narrowed exec (only lane 0 active here).
  ; Inactive lanes (1..63 of the wave) would, in correct codegen,
  ; receive 0x55555555 via cndmask. After AND 0xFFFF + strict.wwm +
  ; readlane(., 1), lane 0 should observe 0x5555.
  ;
  ; At -O2 the SimplifyDemandedBits target hook folds the call to
  ; constant 0xAAAAAAAA, the AND becomes 0xAAAA, strict.wwm and
  ; readlane fold through, and lane 0 stores 0xAAAA.
  %v = call i32 @llvm.amdgcn.set.inactive.i32(i32 -1431655766, i32 1431655765)
  %ze = and i32 %v, 65535
  %w = call i32 @llvm.amdgcn.strict.wwm.i32(i32 %ze)
  %x = call i32 @llvm.amdgcn.readlane(i32 %w, i32 1)
  store i32 %x, ptr addrspace(1) %out, align 4
  br label %ret

ret:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x() #1
declare i32 @llvm.amdgcn.set.inactive.i32(i32, i32) #2
declare i32 @llvm.amdgcn.strict.wwm.i32(i32) #2
declare i32 @llvm.amdgcn.readlane(i32, i32) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(none) }
attributes #2 = { convergent nocallback nofree nosync nounwind willreturn memory(none) }
