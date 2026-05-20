# m087: `simplifyAMDGCNMemoryIntrinsicDemanded` drops the high channel of an `image_store` with sparse DMask

*Discovery method: code inspection.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUInstCombineIntrinsic.cpp:2317-2342` (the channel-trimming loop reached from the `image_store_*` case at lines 2107-2126).

The helper `trimTrailingZerosInVector` (or `defaultComponentBroadcast`
on GFX12+) computes `DemandedElts` as a **contiguous prefix** of the
vector lanes whose values are not the format's default (`0.0` for
pre-GFX12 "default zero", or the broadcast value for GFX12).  The
shared channel-trimming loop then walks `DMask` bits left-to-right and
drops every set DMask bit whose position-among-set-bits is past that
prefix.

For a *sparse* DMask such as `0b1010` (Y + W) and `vdata = <Y_val, 0>`:

* `trimTrailingZerosInVector` says lane 1 is zero → demanded = `0b01`.
* The DMask loop walks bits 0..3.  Bit 1 (the Y channel, first set in
  DMask) is kept; bit 3 (W, second set in DMask) is dropped because
  there is no lane 1 in the demanded mask.
* New DMask = `0b0010` (Y only); the call's vdata is shrunk from
  `<a, 0>` to `<a>`.

This is wrong for **stores**.  The pre-GFX12 "default zero" / GFX12
"default broadcast" semantics only fill in *missing source dwords of
channels that remain in DMask*; they do not compensate for channels
removed from DMask altogether.  A channel that was being written to
the image with the value `0.0` is now simply not written -- the
existing memory at that channel is left unchanged.

The same logic for `image_load` is sound because removing a channel
from DMask just means the loader returns the format-default for that
result lane, which matches `<a, 0>` exactly.

## Why existing LIT tests miss this

`llvm/test/CodeGen/AMDGPU/amdgcn-simplify-image-buffer-stores.ll` only
exercises *contiguous* DMasks (`0x1`, `0x7`, `0xF`), where the
"position-among-set-bits" matches the lane index and the trim is
sound.

Buffer-format / tbuffer stores use a different code path (no DMask)
and are unaffected.

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdpal"

declare void @llvm.amdgcn.image.store.1d.v2f32.i32.v8i32(<2 x float>, i32 immarg, i32, <8 x i32>, i32 immarg, i32 immarg)

define amdgpu_ps void @sparse_dmask_trailing_zero(<8 x i32> inreg %rsrc, float %a, i32 %s) {
  %v0 = insertelement <2 x float> poison, float %a, i32 0
  %v1 = insertelement <2 x float> %v0, float 0.0, i32 1
  ; DMask = 0b1010 = 10 -- write Y and W only
  call void @llvm.amdgcn.image.store.1d.v2f32.i32.v8i32(<2 x float> %v1, i32 10, i32 %s, <8 x i32> %rsrc, i32 0, i32 0)
  ret void
}
```

## Asm-level demonstration (gfx950, amdpal)

```bash
clang -O0 -target amdgcn-amd-amdpal -mcpu=gfx950 -nogpulib -S \
    -x ir amdgpu/known-miscompiles/m087-image-store-sparse-dmask-trim/reduced.ll \
    -o /tmp/o0.s
clang -O2 -target amdgcn-amd-amdpal -mcpu=gfx950 -nogpulib -S \
    -x ir amdgpu/known-miscompiles/m087-image-store-sparse-dmask-trim/reduced.ll \
    -o /tmp/o2.s
```

`-O0` (correct):

```asm
image_store v[0:1], v2, s[0:7] dmask:0xa unorm   ; writes Y=a, W=0
```

`-O2` (BUG):

```asm
image_store v0, v2, s[0:7] dmask:0x2 unorm       ; writes only Y; W left unchanged
```

The W channel of the target image will retain whatever value was
previously there instead of being set to `0.0`.

Reproduces identically on `mcpu=gfx1200` (the GFX12 "default broadcast"
path) with `vdata = <a, a>`: the broadcast-prefix shrinks the demanded
mask the same way, and DMask `0xa` is rewritten to `0x2`.

## How a fix should look

The channel-trimming loop must distinguish *load* from *store*.  For
stores, demanded-elements analysis can only justify *removing trailing
DMask channels whose source value equals the format's default for that
channel*, AND only when those channels are also trailing in
`vdata`/source layout.  Concretely: skip the DMask-bit drop when the
intrinsic is a store with a non-contiguous DMask whose trimmed
channels would change which image channels get written.

## Why the FuzzX runtime harness doesn't catch it

The standard `run_ll_reproducer.sh` only drives `amdgpu_kernel` HSA
compute kernels.  `image_store` requires an `amdgpu_ps` shader with
image-descriptor SGPRs that the HIP module runner cannot construct.
Demonstrated at the IR (`opt -passes=instcombine` rewrites DMask
`0xa` → `0x2` and shrinks vdata) and asm (`dmask:0x2` instead of
`dmask:0xa`) levels.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`dmask:0x2`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Reproduces. |
