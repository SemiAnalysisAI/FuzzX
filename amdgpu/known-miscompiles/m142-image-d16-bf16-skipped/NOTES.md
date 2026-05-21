# m142: `lowerImage` D16 detection uses `MVT::f16` only, silently mishandles `<N x bfloat>` image data

*Discovery method: code inspection (during amdgcn.image bf16/f16 audit).*

Sibling shape to c008 (amdgcn.class.bf16 ISel ICE) and m118
(`isCanonicalized` bf16 over-promise) -- AMDGPU SDAG treats f16 as
the only "16-bit float" while bf16 is silently a corner case.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp`:
* line **10190** (store path): `IsD16 = ... &&getScalarType() == MVT::f16`
* line **10203** (load path): same predicate
* line **11926** (`handleD16VData` driver)
* lines 12056, 12084, 12120, 12170 (TBUFFER store paths)

The D16 detection compares the scalar element type against
`MVT::f16` *only*.  bf16 is a distinct `MVT` (`MVT::bf16`), so a
`<N x bfloat>` image data/result silently:

* **Store path:** `IsD16 = false`, skips `handleD16VData`, computes
  `NumVDataDwords = ceil(bytes / 32)` from the 16-bit-element
  vector.  The selected MIMG opcode is the non-D16 variant fed with
  a half-sized VReg, producing wrong dword layout/encoding.  The
  `HasD16` guard at 10191 is also bypassed -- no diagnostic.

* **Load path:** same -- non-D16 MIMG opcode selected,
  `NumVDataDwords = DMaskLanes` (not halved), `constructRetValue`
  (line 9995) widens to a `v*i32` then bitcasts to `<N x bfloat>`.
  Payload bits are wrong.

GISel does the right thing -- `AMDGPULegalizerInfo::legalizeImageIntrinsic`
line **7182** uses `Ty.getScalarType() == S16`, which covers both
half AND bfloat.  The SDAG/GISel divergence makes this an
**SDAG-only miscompile**: same IR yields different image opcodes
and different result bits depending on `-global-isel=0` vs `=1`.

## Reproducer

`reduced.ll`:

```llvm
declare <4 x bfloat> @llvm.amdgcn.image.sample.2d.v4bf16.f32(
    i32, float, float, <8 x i32>, <4 x i32>, i1, i32, i32)

define amdgpu_ps <4 x bfloat> @t(<8 x i32> inreg %r, <4 x i32> inreg %s,
                                 float %x, float %y) {
  %v = call <4 x bfloat> @llvm.amdgcn.image.sample.2d.v4bf16.f32(
         i32 15, float %x, float %y, <8 x i32> %r, <4 x i32> %s,
         i1 0, i32 0, i32 0)
  ret <4 x bfloat> %v
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -global-isel=0 -O2 reduced.ll`:
emits the **non-D16** form (`image_sample` without `d16`), with a
mismatched VReg width.  `llc -global-isel=1` correctly emits
`image_sample ... d16`.

## Suggested fix

Replace `getScalarType() == MVT::f16` with `getScalarType().getSizeInBits() == 16`
in all six SDAG sites:

* SIISelLowering.cpp:10190
* SIISelLowering.cpp:10203
* SIISelLowering.cpp:11926
* SIISelLowering.cpp:12056
* SIISelLowering.cpp:12084
* SIISelLowering.cpp:12120
* SIISelLowering.cpp:12170

This matches the `IsD16` shape at SIISelLowering.cpp:7715 (which
already does the right thing).  The GISel sibling at line 7182
uses the same pattern.

Optional follow-ups (lower priority, not the silent miscompile):
* `lowerImage`'s A16-bias check at line 10245
* `GradPackVectorVT` / `AddrPackVectorVT` at lines 10235/10240
  for bf16 *coordinates* (rare use case).

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `amdgcn.image.*.v?bf16.*` intrinsics.
  Per `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should add image intrinsics with bf16 data overloads.
* No upstream lit test exercises bf16 image intrinsics -- `grep
  bfloat llvm/test/CodeGen/AMDGPU/llvm.amdgcn.image.{d16,load,store,
  sample,gather4}.*.ll` returns zero hits.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | SDAG path code present; same divergence as upstream. |
| ROCm 7.1.1 | Same defect. |
