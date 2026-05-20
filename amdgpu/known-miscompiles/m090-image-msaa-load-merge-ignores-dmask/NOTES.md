# m090: `AMDGPUImageIntrinsicOptimizer` merges `image_load_2dmsaa` calls with different `DMask`s

*Discovery method: code inspection.* Sibling shape to m087 (DMask trimming) -- another DMask-related image-intrinsic bug.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUImageIntrinsicOptimizer.cpp:114`:

```cpp
// Check all arguments (DMask, VAddr, RSrc etc) are identical.
for (int I = 1, E = II->arg_size(); AllEqual && I != E; ++I) {
  AllEqual &= II->getArgOperand(I) == IIList.front()->getArgOperand(I);
}
```

The in-source comment claims "Check all arguments (DMask, VAddr, RSrc etc)".
The implementation does not: `I` starts at `1`, but `ImageDimIntr->DMaskIndex == 0`
for every `image_load_*` intrinsic, so the DMask argument is silently skipped.

`optimizeSection` (lines 203-205) then takes `DMask` from `IIList.front()`
and bakes it into the merged `image_msaa_load`, discarding every other
call's DMask.  The per-call extracts at lines 251-266 use lane
`Idx.urem(4)`, assuming all merged loads share the same channel layout.

Net effect: two `image_load_2dmsaa` calls at the same coords with
different DMasks (say `dmask=0x1`/R and `dmask=0x8`/A) get fused into a
single `image_msaa_load` with `dmask=0x1` returning `<R(f0), R(f1),
R(f2), R(f3)>`.  The second extract reads `R(f1)` even though the
original program asked for `A(f1)`.  The A channel for the second
fragment is never loaded.

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdpal"

declare float @llvm.amdgcn.image.load.2dmsaa.f32.i32(i32 immarg, i32, i32, i32, <8 x i32>, i32 immarg, i32 immarg)

define amdgpu_ps <2 x float> @dmask_mismatch_merge(<8 x i32> inreg %rsrc, i32 %s, i32 %t) {
  %a = call float @llvm.amdgcn.image.load.2dmsaa.f32.i32(i32 1, i32 %s, i32 %t, i32 0, <8 x i32> %rsrc, i32 0, i32 0)  ; DMask R, fragId 0
  %b = call float @llvm.amdgcn.image.load.2dmsaa.f32.i32(i32 8, i32 %s, i32 %t, i32 1, <8 x i32> %rsrc, i32 0, i32 0)  ; DMask A, fragId 1
  %v0 = insertelement <2 x float> poison, float %a, i32 0
  %v1 = insertelement <2 x float> %v0,    float %b, i32 1
  ret <2 x float> %v1
}
```

## Demonstration via opt

```bash
/opt/rocm-7.1.1/lib/llvm/bin/opt -S -passes=amdgpu-image-intrinsic-opt \
    -mtriple=amdgcn-amd-amdpal -mcpu=gfx1150 \
    amdgpu/known-miscompiles/m090-image-msaa-load-merge-ignores-dmask/reduced.ll
```

Output (key lines):

```llvm
%1 = call <4 x float> @llvm.amdgcn.image.msaa.load.2dmsaa.v4f32.i32.v8i32(
        i32 1,                  ; <-- dmask=0x1 (R only); the dmask=0x8 (A) of %b is gone
        i32 %s, i32 %t, i32 0,  ; coord, fragId=0 (the front call's fragId)
        <8 x i32> %rsrc, i32 0, i32 0)
```

The pre-pass IR had two distinct calls with `dmask=0x1` and `dmask=0x8`;
the post-pass IR has a single fused call with `dmask=0x1`, and `%b` is
reconstructed via `extractelement` from this single load.

## Why this doesn't fire on gfx950

The default `MCPU=gfx950` happens to have the `MSAALoadDstSelBug`
subtarget feature set, which causes `AMDGPUImageIntrinsicOptimizer` to
bail out early (subtarget gate in the pass entry).  The reproducer
therefore uses `-mcpu=gfx1150` (RDNA3.5) where the gate is open.  Any
gfx10/gfx11 wave-graphics shader with this idiom is at risk on the
default `clang -O2` pipeline.

## How a fix should look

Change the loop bound:

```cpp
for (int I = 0, E = II->arg_size(); AllEqual && I != E; ++I) {
  AllEqual &= II->getArgOperand(I) == IIList.front()->getArgOperand(I);
}
```

`FragIdIndex` is the only operand whose mismatch *should* be allowed
within a group; the existing loop body at lines 105-112 already excludes
it by skipping when `I == FragIdIndex`.  Starting at 0 will include the
DMask check too.  The same applies symmetrically to any future
`image_load_*` whose operand 0 is something other than DMask -- the
guard `I == FragIdIndex` is the right way to express "all args equal
except FragId".

## Sibling-bug candidates ruled out

* TFE / R128 mismatch: TFE for `image_load_2dmsaa` is folded into
  `tex_fail_ctrl` at arg index 5, which IS compared by the loop.
* `image_store` aliasing in between: `collectMergeableInsts` (line 148)
  bails on `mayHaveSideEffects()`, which covers `image_store`
  (`IntrWriteMem`).
* Wrong FragId reconstruction: `Idx.urem(4)` at line 256/263 correctly
  recovers the original lane each call requested *within its group* --
  conditional on DMask being equal, which is the bug above.
* Coverage / sparse load mixing: `getIntrinsicID()` is checked at line
  104, so `2dmsaa` and `2darraymsaa` can't be in the same group.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`<4 x float> ... dmask=1` for two distinct-DMask calls). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Reproduces. |

Pass runs at `clang -O2` for any gfx target where
`MSAALoadDstSelBug` is unset (gfx10.0 / gfx10.1 / gfx10.3 / gfx11.5+),
demonstrated above on `gfx1150`.
