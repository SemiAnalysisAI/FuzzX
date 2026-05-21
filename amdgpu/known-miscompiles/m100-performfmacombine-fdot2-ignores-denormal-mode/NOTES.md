# m100: `performFMACombine` FDOT2 fold ignores `denormal-fp-math-f32`, switching from mode-respecting `v_fma_mix_f32` to always-FTZ `v_dot2c_f32_f16`

*Discovery method: code inspection.* Sibling shape to m075/m077/m093/m094/m095 (FP-mode/sign-of-zero family) but distinct: this one is denormal-mode rather than sign-of-zero.

## The bug

`SIISelLowering.cpp:17729-17800` (`SITargetLowering::performFMACombine`)
folds the FMA-chain pattern
`fma(fpext(a.x), fpext(b.x), fma(fpext(a.y), fpext(b.y), z))` into
`AMDGPUISD::FDOT2`.  The fold is gated only on contract:

```cpp
if (!(Options.AllowFPOpFusion == FPOpFusion::Fast) &&
    !(N->getFlags().hasAllowContract() &&
      FMA->getFlags().hasAllowContract()))
  return SDValue();
```

The in-source comment justifies the contract-only guard as:

> // fdot2_f32_f16 always flushes fp32 denormal operand and output to zero,
> // regardless of the denorm mode setting. Therefore, allowing this fold
> // when fp-contract is sufficient since it does not regress denorm-flush
> // ...

That conflates two orthogonal properties.  `contract` is purely a
fusion permit; it does NOT license flushing denormals.  A kernel
compiled with `"denormal-fp-math-f32"="ieee,ieee"` (mode 3,
denormal-preserving) and `-O2` on gfx950 will silently switch from
`v_fma_mix_f32` (mode-respecting) to `v_dot2c_f32_f16_e32` (always
FTZ), even though `dot10-insts` is the gfx950 default feature set.

The selected instruction differs in observable FP behaviour: any
intermediate f32 subnormal produced by the chain that the source
program asked the compiler to preserve gets silently flushed to `±0`.

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @t(ptr addrspace(1) %out,
                              <2 x half> %a, <2 x half> %b, float %z) #0 {
  %ax = extractelement <2 x half> %a, i32 0
  %ay = extractelement <2 x half> %a, i32 1
  %bx = extractelement <2 x half> %b, i32 0
  %by = extractelement <2 x half> %b, i32 1
  %axf = fpext half %ax to float
  %ayf = fpext half %ay to float
  %bxf = fpext half %bx to float
  %byf = fpext half %by to float
  %inner = call contract float @llvm.fma.f32(float %ayf, float %byf, float %z)
  %outer = call contract float @llvm.fma.f32(float %axf, float %bxf, float %inner)
  store float %outer, ptr addrspace(1) %out
  ret void
}

declare float @llvm.fma.f32(float, float, float)

attributes #0 = { "denormal-fp-math-f32"="ieee,ieee" }
```

Codegen with `llc -mcpu=gfx950 -O2`:

* Default (`+dot10-insts`): selects `v_dot2c_f32_f16_e32 v2, s2, v1` --
  always FTZ.
* With `-mattr=-dot10-insts`: selects `v_fma_mix_f32 ...` x2 -- honours
  `.amdhsa_float_denorm_mode_32 3` (the IEEE-preserve setting the
  kernel asked for).

Both compiles emit the same `.amdhsa_float_denorm_mode_32 3` header
asserting the kernel preserves f32 subnormals.  The FDOT2 form ignores
that assertion at runtime.

## How a fix should look

`contract` is the wrong-only guard.  Either also require the kernel's
f32 denormal output mode to be flushing:

```cpp
const auto FPMode = MF.getDenormalMode(APFloat::IEEEsingle());
if (FPMode.Output != DenormalMode::PreserveSign &&
    FPMode.Output != DenormalMode::IEEE /* really flushing */)
  return SDValue();
```

(actually, for IEEE-mode kernels: bail).  Or require the chain to
carry `nnan ninf nsz` flags as well, where the user has explicitly
opted out of all subnormal/sign-of-zero semantics.

## Why the fuzzer doesn't see it

* FDOT2 requires the specific lane-matched
  `fpext(extractelement v2f16, 0/1)` chain pattern with `contract`
  flags on both FMAs.  The fuzzer's current FP-emit doesn't combine
  packed-half extracts + paired contract-FMAs in the right shape.
* The interpreter oracle is skipped for kernels using `extractelement`
  on `v2f16` patterns this aggressively.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`v_dot2c_f32_f16_e32` emitted under `denormal-fp-math-f32=ieee,ieee`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same fold present. |

Not a HEAD-only regression -- the fold has been in `performFMACombine`
for some time.

## Why no runtime O0/O2 mismatch in the FuzzX harness

The harness invokes `clang -O0` and `clang -O2` end-to-end with the
default `denormal-fp-math-f32` mode (`preserve-sign,preserve-sign`),
under which both forms flush -- no observable divergence.  Setting
`denormal-fp-math-f32=ieee,ieee` and the contract-FMA chain produces
the asm divergence above.
