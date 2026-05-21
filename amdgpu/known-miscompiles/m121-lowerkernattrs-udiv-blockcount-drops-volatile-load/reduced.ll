; m120 reproducer: AMDGPULowerKernelAttributes pre-V5 UDiv -> block_count
; upgrade (file AMDGPULowerKernelAttributes.cpp, lines 421-444) does not check
; isSimple() on the matched dispatch_ptr GRID_SIZE_X load. The matcher
;     m_UDiv(m_ZExtOrSelf(m_Load(m_GEP(dispatch.ptr, GRID_SIZE_X+I*4))), m_Value())
; accepts a *volatile* load. The pass then erases the UDiv and rewires
; consumers to a freshly-built non-volatile load of HIDDEN_BLOCK_COUNT_X
; from implicitarg_ptr.
;
; Net effect: the value produced by the user's volatile load
; ("grid_size_x") is silently discarded; consumers receive
; block_count_x (a different memory location) instead. The volatile load
; itself is preserved as a dead side-effecting op, masking the loss.

target triple = "amdgcn-amd-amdhsa"
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"

declare ptr addrspace(4) @llvm.amdgcn.dispatch.ptr() #0
declare ptr addrspace(4) @llvm.amdgcn.implicitarg.ptr() #0

define amdgpu_kernel void @k(ptr addrspace(1) %out) #1 {
entry:
  %iap = call ptr addrspace(4) @llvm.amdgcn.implicitarg.ptr()
  ; HIDDEN_GROUP_SIZE_X = 12, i16
  %gsp = getelementptr inbounds i8, ptr addrspace(4) %iap, i64 12
  %gs  = load i16, ptr addrspace(4) %gsp, align 2

  %dp  = call ptr addrspace(4) @llvm.amdgcn.dispatch.ptr()
  ; GRID_SIZE_X = 12, i32
  %gxp = getelementptr inbounds i8, ptr addrspace(4) %dp, i64 12
  ; The volatile read the user wrote, expected to be observed as the
  ; numerator of the division.
  %gx  = load volatile i32, ptr addrspace(4) %gxp, align 4

  %gs32 = zext i16 %gs to i32
  ; This UDiv is what the pass match-erases.
  %bcx  = udiv i32 %gx, %gs32

  store i32 %bcx, ptr addrspace(1) %out, align 4
  ret void
}

attributes #0 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #1 = { "uniform-work-group-size"="false" }

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 500}
