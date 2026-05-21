; m096 reproducer: GCNTTIImpl::isAlwaysUniform over-claims when
;
;   match(V, m_c_And(m_Intrinsic<amdgcn_workitem_id_x>, m_Value(Mask)))
;
; matches and Mask's KnownBits has >= log2(wavefrontSize) trailing zeros,
; *without* checking that Mask is itself uniform.
;
; Wave64 (gfx950): wavefrontSizeLog2 = 6.  reqd_work_group_size = (256,1,1)
; gives 4 waves per block.
;
; Per-lane divergent %div is loaded; %mask = shl %div, 6 has 6 trailing
; zeros, satisfying the buggy hook.  %val = %tid & %mask is genuinely
; divergent.  We then ask for lane 0's %val via readlane.
;
; AMDGPUUniformIntrinsicCombine sees %val as "uniform" (because the
; hook pinned it AlwaysUniform), so it deletes the readlane and every
; lane stores its own %val.
;
; RUN-LLVM-BUILD: build/llvm-fuzzer
; ; inputs: lanes 0..63 -> 0, lanes 64..127 alternate 0/1, lanes 128..255 -> 0
; RUN-INPUTS: [0*64, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0*128]
; RUN-COMBINED: true

target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %inp,
                                       ptr addrspace(1) %out) #0
                                       !reqd_work_group_size !0 {
entry:
  %tid   = call i32 @llvm.amdgcn.workitem.id.x()
  %gep   = getelementptr i32, ptr addrspace(1) %inp, i32 %tid
  %div   = load i32, ptr addrspace(1) %gep, align 4
  %mask  = shl i32 %div, 6
  %val   = and i32 %tid, %mask
  ; A correct compile broadcasts lane 0's %val to every lane in the wave.
  ; For wave 1 (tids 64..127) lane 0 has tid=64, mask=64*input[64]=0, so
  ; lane-0's %val = 0; thus the entire wave should observe 0.
  %rfl   = call i32 @llvm.amdgcn.readlane(i32 %val, i32 0)
  %dst   = getelementptr i32, ptr addrspace(1) %out, i32 %tid
  store i32 %rfl, ptr addrspace(1) %dst, align 4
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.amdgcn.readlane(i32, i32)

attributes #0 = { nounwind }

!0 = !{i32 256, i32 1, i32 1}
