; memcpy/memset on LDS pointer — does the pass leave addrspace(3) accesses?
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

@lds.buf = internal addrspace(3) global [16 x i8] poison, align 4

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %src) #0 {
entry:
  %p = getelementptr [16 x i8], ptr addrspace(3) @lds.buf, i32 0, i32 0
  call void @llvm.memset.p3.i32(ptr addrspace(3) %p, i8 0, i32 16, i1 false)
  call void @llvm.memcpy.p3.p1.i32(ptr addrspace(3) %p, ptr addrspace(1) %src, i32 16, i1 false)
  ret void
}

declare void @llvm.memset.p3.i32(ptr addrspace(3) nocapture writeonly, i8, i32, i1)
declare void @llvm.memcpy.p3.p1.i32(ptr addrspace(3) nocapture writeonly, ptr addrspace(1) nocapture readonly, i32, i1)

attributes #0 = { sanitize_address "amdgpu-flat-work-group-size"="1,1024" "target-cpu"="gfx950" }

!llvm.module.flags = !{!0, !1}
!0 = !{i32 4, !"nosanitize_address", i32 1}
!1 = !{i32 1, !"amdhsa_code_object_version", i32 500}
