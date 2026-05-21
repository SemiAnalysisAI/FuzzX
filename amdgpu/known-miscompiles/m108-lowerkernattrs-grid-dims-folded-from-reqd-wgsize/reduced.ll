; Reproduces miscompile of get_work_dim() / hidden_grid_dims load:
; AMDGPULowerKernelAttributes
; (AMDGPULowerKernelAttributes.cpp:121-142, 146-158, 271-274) replaces
; a load from `implicitarg.ptr + 64` (hidden_grid_dims) with a constant
; derived from the kernel's `!reqd_work_group_size` metadata.
;
;   computeNumGridDims:
;     reqd_work_group_size(N, 1, 1) -> 1
;     reqd_work_group_size(N, M, 1) -> 2
;     otherwise                     -> 3
;
; But `hidden_grid_dims` is *dispatch-time* data -- it's `setup.DIMENSIONS`
; from the AQL packet, set by the runtime when the kernel is launched.
; OpenCL/HIP explicitly allow dispatching a kernel with
; `reqd_work_group_size(8,1,1)` as a 2-D or 3-D NDRange (with
; grid_size_{y,z}=1, group_size_{y,z}=1).
;
; AMDGPUUsage.rst:5358 states: "hidden_grid_dims ... the same value as
; the AQL dispatch packet dimensionality." That can be 1, 2, or 3 at
; runtime regardless of `reqd_work_group_size`.
;
; The fold makes `get_work_dim()` (which loads hidden_grid_dims) return
; the wrong value for any kernel with `reqd_work_group_size(N,1,1)`
; dispatched as 2-D or 3-D.
;
; This reproducer is at the IR / opt level (not -O0 vs -O2 runtime) since
; the fold completely deletes the dispatch-time load.
;
; Run with:
;   opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 \
;       --amdhsa-code-object-version=5 \
;       -passes=amdgpu-lower-kernel-attributes,instcombine -S reduced.ll

source_filename = "m108-lowerkernattrs-grid-dims-folded-from-reqd-wgsize"
target triple = "amdgcn-amd-amdhsa"

declare ptr addrspace(4) @llvm.amdgcn.implicitarg.ptr()

define amdgpu_kernel void @reqd_811_reads_grid_dims(ptr addrspace(1) %out)
    !reqd_work_group_size !0 {
  %iap = call ptr addrspace(4) @llvm.amdgcn.implicitarg.ptr()
  %gd_gep = getelementptr inbounds i8, ptr addrspace(4) %iap, i64 64
  %gd = load i16, ptr addrspace(4) %gd_gep, align 2
  store i16 %gd, ptr addrspace(1) %out, align 2
  ret void
}

; reqd_work_group_size = (8, 1, 1)  -- per the kernel-static metadata.
!0 = !{i32 8, i32 1, i32 1}

; COV5 required so the pass uses ImplicitArgOffsets (block_count_* etc.).
!llvm.module.flags = !{!1}
!1 = !{i32 1, !"amdhsa_code_object_version", i32 500}
