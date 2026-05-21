# m122: `LowerFDIV64` lacks denormal-mode handling -- f64 fdiv NR chain runs under FTZ when the kernel attributes ask for IEEE denormals

*Discovery method: code inspection.*  Sibling shape to m075/m077/m104
(denormal-mode-blind FP arithmetic) -- this is the f64-fdiv SDAG
lowering counterpart.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:13471-13538`
(`LowerFDIV64`).

`LowerFDIV32` (same file, 13334-13468) explicitly wraps its NR chain
in `S_SETREG_B32 / DENORM_MODE` writes to force IEEE denormals around
the four `v_fma_f32` refinement steps, saving and restoring the mode:

```cpp
// LowerFDIV32 (excerpt around 13364-13416, 13436-13462):
SDValue PreSetreg = DAG.getNode(AMDGPUISD::DENORM_MODE, ..., SetReg32, ...);
// ... NR FMA chain ...
SDValue PostSetreg = DAG.getNode(AMDGPUISD::DENORM_MODE, ..., RestoreReg32, ...);
```

`LowerFDIV64` performs the equivalent f64 NR chain
(`DIV_SCALE / FMA*4 / FMUL / DIV_FMAS / DIV_FIXUP`) with **no**
analogous denormal-mode toggle:

```cpp
// LowerFDIV64 (13471-13538):
SDValue Scale0 = DAG.getNode(AMDGPUISD::DIV_SCALE, SL, ScaleVT, Numerator, Denominator, ...);
SDValue Rcp = DAG.getNode(AMDGPUISD::RCP, SL, MVT::f64, Scale0.getValue(0));
// ... bunch of v_fma_f64 refinement steps, NO mode toggle ...
SDValue Fixup = DAG.getNode(AMDGPUISD::DIV_FIXUP, SL, MVT::f64, ...);
return Fixup;
```

Under `denormal-fp-math="preserve-sign,preserve-sign"` (a legal
kernel attribute affecting `FP64FP16Denormals`), the `v_fma_f64`
refinement runs with f64 FTZ.  Any intermediate that lands in the
f64 subnormal range (`< 2^-1022`) is silently flushed to ±0,
breaking NR convergence.  Result: wrong quotient for divisors near
`2^-1022` (and symmetrically for very large divisors).

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %out, double %x, double %y) #0 {
  %r = fdiv double %x, %y
  store double %r, ptr addrspace(1) %out
  ret void
}
attributes #0 = { "denormal-fp-math"="preserve-sign,preserve-sign" "target-cpu"="gfx950" }
```

Codegen with `clang -O0` and `clang -O2` (`-mcpu=gfx950`) produces
**identical** asm -- no `s_setreg DENORM_MODE` toggles around the NR
chain.  Compare `LowerFDIV32` asm which inserts:

```asm
s_setreg_imm32_b32 hwreg(HW_REG_MODE, 4, 2), 3   ; pre-NR: force IEEE
... v_div_scale_f32, v_fma_f32 x4, v_fmul_f32, v_div_fmas_f32 ...
s_setreg_imm32_b32 hwreg(HW_REG_MODE, 4, 2), 0   ; post-NR: restore
v_div_fixup_f32
```

The f64 equivalent prints just the NR sequence with no mode toggle.

## Why no runtime O0/O2 mismatch

The combiners that would create the divergence run only at -O2 but
this is a Custom *legalization* (always runs).  Both opt levels emit
the same buggy lowering.  The FuzzX O0-vs-O2 oracle reports
`any_mismatch=false`.

The witness is SDAG-vs-IR-semantics (or a host f64 interpreter), or
SDAG-vs-GISel (the GISel f64 fdiv lowering goes through different
legalization).

## Suggested fix

Mirror `LowerFDIV32` lines 13364-13416 / 13436-13462 for f64.  Check
`MFI.getMode().FP64FP16Denormals` (analogous to the f32 path's
`FP32Denormals`); if it indicates FTZ, emit
`S_SETREG_B32 HW_REG_MODE_denorm_64_FFFFFFFFFFFFFFFF` (or similar)
before the NR chain and restore after.

Also set `Flags.setNoFPExcept(true)` to match the f32 path's
strict-FP discipline at `SIISelLowering.cpp:13342-13343`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces -- no `s_setreg DENORM_MODE` in `LowerFDIV64` output. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same lowering, same defect. |

## Why the fuzzer hasn't caught it

* Default kernel attributes don't set
  `denormal-fp-math="preserve-sign"` for f64.
* The IR fuzzer rarely emits `fdiv double` with divisors in the
  `2^-1022 .. 2^-1015` near-denormal range.
* The interpreter oracle is currently skipped for f64 fdiv.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  weight `2^-1022`, `2^-1023`, and `2^1022` higher in the f64
  constant pool and emit kernels with mixed `preserve-sign` /
  `ieee` denormal modes on f64.
