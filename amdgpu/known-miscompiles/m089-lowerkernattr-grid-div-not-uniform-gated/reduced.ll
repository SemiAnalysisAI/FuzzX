; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0,0,0
;
; AMDGPULowerKernelAttributes rewrites
;   udiv(grid_size_x, group_size_x)        ; floor division
; into
;   load_implicit_arg(HIDDEN_BLOCK_COUNT_X) ; ceil division (per AMDHSA ABI)
; without checking the `uniform-work-group-size` function attribute.
;
; When the grid size is a multiple of the group size (true for every HIP
; launch and for OpenCL <2.0 / OpenCL >=2.0 with uniform work-groups), the
; rewrite is a no-op at runtime.  When the launch is non-uniform
; (OpenCL >=2.0 with `-cl-uniform-work-group-size=false`, or hand-built
; dispatch packets) the two values differ: floor(grid/group) and
; ceil(grid/group) disagree whenever grid % group != 0.
;
; The HIP `hipModuleLaunchKernel` runtime always sets
; dispatch.grid_size = gridDim * blockDim, so the harness cannot observe a
; runtime divergence under HIP; this is a *latent* miscompile rather than a
; harness-reproducible one.  Demonstration is at the IR level.

target triple = "amdgcn-amd-amdhsa"

declare ptr addrspace(4) @llvm.amdgcn.implicitarg.ptr()
declare ptr addrspace(4) @llvm.amdgcn.dispatch.ptr()

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
entry:
  %ip = call ptr addrspace(4) @llvm.amdgcn.implicitarg.ptr()
  ; HIDDEN_GROUP_SIZE_X is at offset 12, size i16
  %gsx_ptr = getelementptr i8, ptr addrspace(4) %ip, i64 12
  %gsx = load i16, ptr addrspace(4) %gsx_ptr, align 4
  %gsx32 = zext i16 %gsx to i32

  %dp = call ptr addrspace(4) @llvm.amdgcn.dispatch.ptr()
  ; GRID_SIZE_X is at offset 12, size i32
  %grx_ptr = getelementptr i8, ptr addrspace(4) %dp, i64 12
  %grx = load i32, ptr addrspace(4) %grx_ptr, align 4

  ; floor(grid_size_x / group_size_x).  No uniform-work-group-size attr,
  ; so the pass has no right to assume floor==ceil.
  %ng = udiv i32 %grx, %gsx32

  store i32 %ng, ptr addrspace(1) %out, align 4
  ret void
}

attributes #0 = { nounwind "target-cpu"="gfx950" }

!llvm.module.flags = !{!0, !1}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 500}
!1 = !{i32 1, !"wchar_size", i32 4}
