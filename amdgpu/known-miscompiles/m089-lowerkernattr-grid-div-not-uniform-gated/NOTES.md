# m089: AMDGPULowerKernelAttributes rewrites `udiv(grid_size, group_size)` to `block_count` without checking `uniform-work-group-size`

*Discovery method: code inspection.*

## The bug

`AMDGPULowerKernelAttributes.cpp:409-446` walks every i16 load from
`HIDDEN_GROUP_SIZE_X/Y/Z` (the "group size" implicit-arg field) and
rewrites any `udiv(load_dispatch_grid_size_x, group_size_x)` into a fresh
`load(implicitarg + HIDDEN_BLOCK_COUNT_X)`:

```cpp
// Upgrade the old method of calculating the block size using the grid size.
// We pattern match any case where the implicit argument group size is the
// divisor to a dispatch packet grid size read of the same dimension.
if (IsV5OrAbove) {
  for (int I = 0; I < 3; I++) {
    Value *GroupSize = GroupSizes[I];
    if (!GroupSize || !GroupSize->getType()->isIntegerTy(16))
      continue;

    for (User *U : GroupSize->users()) {
      Instruction *Inst = cast<Instruction>(U);
      if (isa<ZExtInst>(Inst) && !Inst->use_empty())
        Inst = cast<Instruction>(*Inst->user_begin());

      using namespace llvm::PatternMatch;
      if (!match(Inst,
                 m_UDiv(m_ZExtOrSelf(m_Load(m_GEP(
                            m_Intrinsic<Intrinsic::amdgcn_dispatch_ptr>(),
                            m_SpecificInt(GRID_SIZE_X + I * sizeof(uint32_t))))),
                        m_Value())))
        continue;
      // ... replace Inst with load of HIDDEN_BLOCK_COUNT_{X,Y,Z}
    }
  }
}
```

`udiv(grid_size, group_size)` is **floor** division. The AMDHSA ABI
defines `hidden_block_count_{x,y,z} = ceil(hidden_grid_size /
hidden_group_size)`. These two values disagree whenever `grid_size %
group_size != 0`, i.e. whenever the launch is a non-uniform work-group
dispatch.

The two `HasUniformWorkGroupSize` branches above (lines 310-347 for V5+,
348-403 for pre-V5) are guarded by the `uniform-work-group-size`
attribute — they explicitly assume `grid_size % group_size == 0`. This
udiv-to-block_count rewrite is **not** guarded. It fires whenever the
kernel reads `HIDDEN_GROUP_SIZE_X` and does a matching grid/group udiv,
regardless of the uniform attribute.

This is the kind of bug that lurks until OpenCL non-uniform work-group
dispatch (OpenCL >=2.0 with `-cl-uniform-work-group-size=false`) actually
hits it. Under HIP, the runtime sets
`dispatch.grid_size = gridDim * blockDim`, which is always a multiple of
the group size, so floor == ceil and the bad rewrite is silent.

## Reproducer

`/tmp/findbug/kernattr/reduced.ll` (full contents inline above and on
disk).  Key fragment:

```llvm
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
  %ip = call ptr addrspace(4) @llvm.amdgcn.implicitarg.ptr()
  %gsx_ptr = getelementptr i8, ptr addrspace(4) %ip, i64 12
  %gsx = load i16, ptr addrspace(4) %gsx_ptr, align 4
  %gsx32 = zext i16 %gsx to i32
  %dp = call ptr addrspace(4) @llvm.amdgcn.dispatch.ptr()
  %grx_ptr = getelementptr i8, ptr addrspace(4) %dp, i64 12
  %grx = load i32, ptr addrspace(4) %grx_ptr, align 4
  %ng = udiv i32 %grx, %gsx32          ; FLOOR(grid_x / group_x)
  store i32 %ng, ptr addrspace(1) %out, align 4
  ret void
}
attributes #0 = { nounwind "target-cpu"="gfx950" }   ; no uniform-work-group-size
!0 = !{i32 1, !"amdhsa_code_object_version", i32 500}
```

## Demonstration — pass replaces floor with ceil

```
$ build/llvm-fuzzer/bin/clang -O2 -nogpulib -target amdgcn-amd-amdhsa \
    -mcpu=gfx950 -x ir reduced.ll \
    -mllvm -print-after=amdgpu-lower-kernel-attributes -S -o /dev/null
*** IR Dump After AMDGPULowerKernelAttributesPass on fuzz_kernel ***
define amdgpu_kernel void @fuzz_kernel(...) #1 {
  %ip   = call ... @llvm.amdgcn.implicitarg.ptr()
  %gsx_ptr = getelementptr ... %ip, i64 12
  %gsx  = load i16, ptr addrspace(4) %gsx_ptr, align 4, !range !2
  %gsx32 = zext i16 %gsx to i32
  %dp   = call ptr addrspace(4) @llvm.amdgcn.dispatch.ptr()
  %grx_ptr = getelementptr ... %dp, i64 12
  %grx  = load i32, ptr addrspace(4) %grx_ptr, align 4   ; DEAD now
  %0    = getelementptr inbounds i8, ptr addrspace(4) %ip, i64 0
  %1    = load i32, ptr addrspace(4) %0, align 4,
                 !invariant.load !3, !noundef !3         ; HIDDEN_BLOCK_COUNT_X
  store i32 %1, ptr addrspace(1) %out, align 4           ; <-- ceil, not floor
  ret void
}
```

No `uniform-work-group-size` attribute on `fuzz_kernel`, yet the pass
silently replaced `floor(grx/gsx32)` with the runtime-supplied
`hidden_block_count_x` (which equals `ceil(grx/gsx32)`).

Same behaviour reproduced with:
* `build/llvm-fuzzer/bin/clang` (HEAD with FuzzX patches)
* `build/rocm-7.2.3-extract/.../opt -passes=amdgpu-lower-kernel-attributes`
* `/opt/rocm-7.1.1/lib/llvm/bin/opt -passes=amdgpu-lower-kernel-attributes`

## Runtime divergence under HIP

```
$ amdgpu/known-miscompiles/run_ll_reproducer.sh /tmp/findbug/kernattr/reduced.ll
[0] input=0x00000000 O0=0x00000000 O2=0x00000000 mismatch=false
[1] input=0x00000000 O0=0x00000000 O2=0x00000000 mismatch=false
[2] input=0x00000000 O0=0x00000000 O2=0x00000000 mismatch=false
any_mismatch=false
```

No divergence is observable through the HIP harness, because
`hipModuleLaunchKernel` always sets `dispatch.grid_size_x = gridDim *
blockDim` — i.e. always a multiple of `blockDim`. To trigger the
mis-match at runtime one needs a non-uniform launch (OpenCL 2.0+ with
`-cl-uniform-work-group-size=false`, or a hand-crafted AQL dispatch
packet); both are out of scope for the current harness.

This is therefore a **latent** miscompile of the udiv->block_count
rewrite. It is harness-invisible only because HIP cannot produce the
inputs that distinguish the two values; the IR-level
miscompile-by-construction is unambiguous.

## Fix sketch

Guard the rewrite on `HasUniformWorkGroupSize` (same as the
`workgroup_id < hidden_block_count` rewrite a few lines above), or
prove `grid_size % group_size == 0` from some other source (e.g.
`reqd_work_group_size` Z=Y=1 and X divides grid_size_x, which is not
generally derivable here).

The simplest fix is to wrap the entire `if (IsV5OrAbove)` block at
lines 409-447 in `&& HasUniformWorkGroupSize`.

## Cases ruled out while auditing

* `annotateGridSizeLoadWithRangeMD` (lines 87-103): `[1, MaxNumGroups+1)`
  emitted as `APInt(32, ...)` onto an `isIntegerTy(32)`-checked load.
  `MaxNumGroups + 1` overflow only at `UINT32_MAX`, which is excluded.
  Lower bound 1 is correct (a kernel always has >=1 workgroup per dim).
* `annotateGroupSizeLoadWithRangeMD` (lines 105-119): `[1, 1025)` for
  group size, `[0, 1024)` for remainder, on i16-typed load (checked).
  Upper bound = `getMaxFlatWorkGroupSize()+1-IsRemainder` matches the
  remainder's `< group_size <= 1024` invariant exactly.
* `annotateGridDimsLoadWithRangeMD` (lines 121-142): bit-width >= 3
  required before emitting `APInt(N, 1)..APInt(N, 4)`; RAUW with
  `ConstantInt::get(Load->getType(), KnownNumGridDims)` uses the load's
  own type. Type-matched.
* The V5+ `HasUniformWorkGroupSize` block (lines 310-347): only fires
  when the attribute is set; replaces `workgroup_id < block_count` with
  `true` and `remainder` with `0`, both correct under uniform.
* The pre-V5 `HasUniformWorkGroupSize` block (lines 348-404): only
  fires when the attribute is set.
* The final `reqd_work_group_size` group-size RAUW (lines 453-462): uses
  `ConstantFoldIntegerCast(KnownSize, GroupSize->getType(), false, DL)`
  to size-match; trusts that `reqd_work_group_size` values fit in i16
  (true for any value <=1024 which is the hardware limit).
* `computeNumGridDims` (lines 146-158): guarded by
  `MD->getNumOperands() == 3`; indexes operands 2 (Z) then 1 (Y),
  matching the X/Y/Z encoding.
* `processUse`'s GEP-walking (lines 185-308): each `case` arm checks
  `LoadSize` matches the field width (4 for i32, 2 for i16) before
  recording the load and adding range MD; can't attach the wrong-width
  MD here, unlike the related `AMDGPULowerKernelArguments.cpp` bug
  (m082).
* `runOnModule` (lines 469-488): walks `BasePtr->users()`; processes
  each `CallInst` exactly once via `HandledUses.insert(CI).second`.
