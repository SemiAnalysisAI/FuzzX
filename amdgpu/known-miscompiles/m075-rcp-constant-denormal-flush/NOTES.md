# m075: `amdgcn.rcp.f32(C)` constant folder ignores f32 flush-to-zero, producing a denormal the hardware would have flushed

Found by reading `AMDGPUInstCombineIntrinsic.cpp` around the `amdgcn_rcp`
fold.  The fold for a constant operand computes `1.0 / C` with full
APFloat precision and inserts the result as a `ConstantFP`.  A `TODO`
comment immediately above the fold even calls out the issue:

```cpp
if (const ConstantFP *C = dyn_cast<ConstantFP>(Src)) {
  const APFloat &ArgVal = C->getValueAPF();
  APFloat Val(ArgVal.getSemantics(), 1);
  Val.divide(ArgVal, APFloat::rmNearestTiesToEven);

  // This is more precise than the instruction may give.
  //
  // TODO: The instruction always flushes denormal results (except for f16),
  // should this also?
  return IC.replaceInstUsesWith(II, ConstantFP::get(II.getContext(), Val));
}
```

`v_rcp_f32` on gfx9xx (and on every CDNA target I checked) flushes
denormal results to `±0` when the kernel's f32 denormal mode is
`PreserveSign` (the AMDGPU default).  The IR fold does not consult the
denormal mode, so any constant input whose exact reciprocal lands in the
denormal range produces a value that the hardware would never emit.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m075-rcp-constant-denormal-flush/reduced.ll
```

Observed output (LLVM HEAD with the local PR patches, `gfx950`):

```text
input=0x00000000
O0=0x00000000   ; correct -- v_rcp_f32(2^127) flushes the denormal result
O2=0x00400000   ; wrong -- constant fold returns the exact denormal 2^-127
mismatch=true
```

The trigger value `0x47E0000000000000` is `2.0 * 2^126 = 2^127` encoded
as an f64 for `ConstantFP`; LLVM converts it to the equivalent f32 value
`0x7F000000` before passing to the intrinsic.

## Why the fuzzer doesn't see it

* The fuzzer's directed emitters only ever call `amdgcn.rcp` with
  runtime values, never constants -- the fold is constant-only.
* Even if it did pass a constant, it almost certainly wouldn't pick one
  whose reciprocal lands in the denormal range (`abs(C) > 2^126`).
* The interpreter oracle is skipped for any module containing an
  `amdgcn.*` intrinsic.

## Fix sketch

Either:

* gate the fold on `MF.getDenormalMode(...).Output == DenormalMode::IEEE`
  (i.e. only emit the exact reciprocal when the kernel actually preserves
  denormals); or
* explicitly flush the folded value when the mode flushes, e.g.
  `if (Val.isDenormal() && ModeFlushes) Val.makeZero(false);`.

The `TODO` comment in the existing code points at the same fix.

## Sibling intrinsics

Spot checking the adjacent intrinsics: `amdgcn.sqrt`, `amdgcn.rsq`,
`amdgcn.tanh` (line 1142+) only fold the `undef` case to qNaN, not
constants, so they don't trip this specific bug today.  But the
`amdgcn.log` / `amdgcn.exp2` folder (line 1166+) does compute
`Val.log() / Val.exp()` with APFloat -- worth a separate audit for the
same flush-to-zero mismatch.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build | Reproduces: `O0=0x00000000`, `O2=0x00400000`. |
| LLVM HEAD with the local PR patches | Reproduces: `O0=0x00000000`, `O2=0x00400000`. |
| ROCm HEAD with the same PR patches applied locally | Reproduces: `O0=0x00000000`, `O2=0x00400000`. |
