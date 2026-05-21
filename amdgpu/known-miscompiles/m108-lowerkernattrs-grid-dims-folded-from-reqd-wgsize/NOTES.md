# m108: `AMDGPULowerKernelAttributes` folds `hidden_grid_dims` load from kernel-static `reqd_work_group_size` metadata

*Discovery method: code inspection.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPULowerKernelAttributes.cpp:121-142, 146-158, 271-274`:

```cpp
// computeNumGridDims (line 146-158):
//   reqd_work_group_size(N, 1, 1) -> 1
//   reqd_work_group_size(N, M, 1) -> 2
//   otherwise                     -> 3
//
// processUse (line 271-274) then:
//   Load->replaceAllUsesWith(ConstantInt::get(LoadTy, KnownNumGridDims));
```

The pass replaces the dispatch-time `hidden_grid_dims` load (at
`implicitarg.ptr + 64`, COV5) with a constant derived from the
kernel-static `!reqd_work_group_size` metadata.

But `hidden_grid_dims` is **not** kernel-static.  It is the AQL
dispatch packet's `setup.DIMENSIONS` field -- set by the runtime when
the kernel is launched.

`llvm/docs/AMDGPUUsage.rst:5358` states:

> `hidden_grid_dims` ... the same value as the AQL dispatch packet
> dimensionality.

That dimensionality is 1, 2, or 3 at runtime, independent of the
kernel-static `reqd_work_group_size`.  OpenCL ([cl_khr_3d_image_writes]
and the NDRange dispatch model) and HIP both explicitly permit
dispatching a kernel with `reqd_work_group_size(N, 1, 1)` as a 2-D or
3-D NDRange (with `grid_size_{y,z} = 1`, `group_size_{y,z} = 1`).

When that happens, `get_work_dim()` (which loads `hidden_grid_dims`)
must return the runtime dimensionality (2 or 3), not the
kernel-static derived `1`.

## Reproducer

`reduced.ll`:

```llvm
declare ptr addrspace(4) @llvm.amdgcn.implicitarg.ptr()

define amdgpu_kernel void @reqd_811_reads_grid_dims(ptr addrspace(1) %out)
    !reqd_work_group_size !0 {
  %iap = call ptr addrspace(4) @llvm.amdgcn.implicitarg.ptr()
  %gd_gep = getelementptr inbounds i8, ptr addrspace(4) %iap, i64 64
  %gd = load i16, ptr addrspace(4) %gd_gep, align 2
  store i16 %gd, ptr addrspace(1) %out, align 2
  ret void
}

!0 = !{i32 8, i32 1, i32 1}
!llvm.module.flags = !{!{i32 1, !"amdhsa_code_object_version", i32 500}}
```

`opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950
--amdhsa-code-object-version=5
-passes=amdgpu-lower-kernel-attributes,instcombine -S reduced.ll`:

* Without the pass: the kernel emits a load of i16 from
  `implicitarg.ptr + 64` and stores it.  At runtime, the value is 1,
  2, or 3 depending on how the kernel is dispatched.
* With the pass: the load is replaced by `i16 1` (the
  kernel-static-derived value).  Stores `1` regardless of dispatch
  dimensionality.

## Why this matters for default pipeline

`AMDGPULowerKernelAttributes` is registered in
`AMDGPUTargetMachine.cpp` and runs at `-O2` for both SDAG and GISel
on every AMDGPU target.  Any source that emits a load from
`implicitarg.ptr + 64` (e.g., the OpenCL/HIP `get_work_dim()`
implementations in the libdevice / OCKL) is affected.

The relevant OCKL implementation `_Z12get_work_dimv` in
`/opt/rocm-X/lib/llvm/lib/clang/X/lib/amdgcn/...` lowers to exactly
this `implicitarg.ptr + 64` load.

## Bug #2 (related, in the same file): pre-V5 UDiv->block_count upgrade ignores volatility

`AMDGPULowerKernelAttributes.cpp:421-444`: the "old `grid_size / group_size
-> hidden_block_count`" upgrade uses `m_Load(...)`.  `m_Load` does not
check `isSimple()`, so a `volatile` or `atomic` load of `grid_size_x`
feeding a UDiv would be silently replaced by a non-volatile load of
`hidden_block_count_x`, losing observable side effects.  Not weaponizable
on default pipeline (LangRef requires `volatile` for a reason that's
unusual for implicitarg loads), but a separate latent bug.

Fix: guard with `cast<LoadInst>(...)->isSimple()` after the match.

## Suggested fix

Remove the `hidden_grid_dims` annotator / fold entirely.  Replace
`annotateGridDimsLoadWithRangeMD` (line 121) with no-op for this
offset.  Keep the existing folds for grid_size / workgroup_id (which
are correctly derivable from `reqd_work_group_size` + uniform-WGS).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (in-tree opt) | Reproduces (load replaced by `i16 1`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Same fold present. |
| ROCm 7.2.3 (`/opt/rocm-7.2.3/lib/llvm/bin/opt`) | Older LLVM -- fold NOT yet present.  This is a post-7.2.3 upstream regression. |

## Why the fuzzer hasn't caught it

* The FuzzX harness compiles+runs single-config dispatches (typically
  1-D) and doesn't vary the AQL setup.DIMENSIONS field across runs.
* The IR fuzzer rarely emits a load from `implicitarg.ptr + 64` (no
  `get_work_dim()`-shaped pattern in the emitter inventory).
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  add `get_work_dim()`/`get_global_id()` -shaped loads from
  `implicitarg.ptr + {0,4,8,..,64}` to the kernel-body emitter.
