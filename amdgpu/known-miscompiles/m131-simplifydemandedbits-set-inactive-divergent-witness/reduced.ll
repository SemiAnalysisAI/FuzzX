target triple = "amdgcn-amd-amdhsa"

declare i32 @llvm.amdgcn.set.inactive.i32(i32, i32) #0
declare i32 @llvm.amdgcn.readlane.i32(i32, i32) #0
declare i32 @llvm.amdgcn.workitem.id.x() #1

; Witness for the latent set_inactive known-bits bug at
; AMDGPUISelLowering.cpp:5841: SimplifyDemandedBitsForTargetNode treats
; amdgcn_set_inactive as a 1-source op and propagates Known from operand(1)
; only, ignoring operand(2) (the value held by inactive lanes).
;
; Construction:
;   - Divergent branch puts only lanes (id >= 32) into if.then; lanes 0..31
;     are EXEC-inactive in this region.
;   - %v = set_inactive(active = id & 0xFF, inactive = 0xFFFF0000)
;       Per V_SET_INACTIVE_B32 lowering (SIWholeQuadMode), the underlying
;       VGPR is filled in WWM with the inactive value for EXEC-off lanes
;       and with %active for EXEC-on lanes.
;   - readlane(%v, 0) reads lane 0's physical VGPR. Lane 0 is inactive in
;     if.then, so its physical content is 0xFFFF0000.
;   - lshr i32 ..., 16 should therefore produce 0xFFFF on every active lane.
;
; Buggy fold: SimplifyDemandedBits propagates Known from set_inactive
; operand(1) = (id & 0xFF), concluding the high 24 bits are zero.
; readlane is also handled the same way (recurses to set_inactive
; operand(1)). The DAGCombiner then folds `lshr ..., 16` to 0.
;
; Reference operand-swap (constant in operand(1)) confirms the correct
; result is 0xFFFF, not 0.
define amdgpu_kernel void @test_buggy(ptr addrspace(1) %out) {
entry:
  %id = call i32 @llvm.amdgcn.workitem.id.x()
  %cond = icmp uge i32 %id, 32
  %active = and i32 %id, 255
  br i1 %cond, label %if.then, label %if.end

if.then:
  %v = call i32 @llvm.amdgcn.set.inactive.i32(i32 %active, i32 -65536)
  %r = call i32 @llvm.amdgcn.readlane.i32(i32 %v, i32 0)
  %hi = lshr i32 %r, 16
  %gep = getelementptr inbounds i32, ptr addrspace(1) %out, i32 %id
  store i32 %hi, ptr addrspace(1) %gep, align 4
  br label %if.end

if.end:
  ret void
}

; Reference: identical shape but with the inactive value put in operand(1).
; The buggy fold then propagates Known from the constant (high bits set) and
; correctly leaves `lshr ..., 16` alone -- yielding `v_mov_b32 v1, 0xffff`.
define amdgpu_kernel void @test_ref(ptr addrspace(1) %out) {
entry:
  %id = call i32 @llvm.amdgcn.workitem.id.x()
  %cond = icmp uge i32 %id, 32
  %active = and i32 %id, 255
  br i1 %cond, label %if.then, label %if.end

if.then:
  %v = call i32 @llvm.amdgcn.set.inactive.i32(i32 -65536, i32 %active)
  %r = call i32 @llvm.amdgcn.readlane.i32(i32 %v, i32 0)
  %hi = lshr i32 %r, 16
  %gep = getelementptr inbounds i32, ptr addrspace(1) %out, i32 %id
  store i32 %hi, ptr addrspace(1) %gep, align 4
  br label %if.end

if.end:
  ret void
}

attributes #0 = { convergent nocallback nofree nounwind willreturn memory(none) }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
