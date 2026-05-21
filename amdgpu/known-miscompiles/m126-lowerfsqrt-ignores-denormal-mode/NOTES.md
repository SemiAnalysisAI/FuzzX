# m126: `lowerFSQRTF32` and `lowerFSQRTF64` lack denormal-mode toggle around NR refinement chains (sibling of m122 for sqrt)

*Discovery method: code inspection.*  Sibling shape to m075/m077/m104/m122
(special-value/denormal-blind FP lowering).

## The bug

### Part 1: f32 sqrt
`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:13682-13770`
(`lowerFSQRTF32`).

Both branches of `lowerFSQRTF32` emit `v_fma_f32` NR/residual chains
with NO `AMDGPUISD::DENORM_MODE` toggle.  Compare `LowerFDIV32`
(`13334-13468`) which wraps its NR chain in `S_SETREG_B32 /
DENORM_MODE` writes (lines 13379-13416 / 13436-13462) to force IEEE
denormals around the FMAs, saving and restoring under
`denormal-fp-math-f32="preserve-sign,..."`.

* THEN branch (`needsDenormHandlingF32 == true`, 13706-13739):
  "adjacent bit pattern" path uses `v_sqrt_f32` + integer add ±1
  ULP + two `v_fma_f32` residual checks.  Subnormal residuals under
  FTZ become 0 -> `SETOLE(0,0) == TRUE` -> silently picks
  `SqrtNextDown`, biasing the result down by 1 ULP.
* ELSE branch (`needsDenormHandlingF32 == false`, 13740-13757): Heron
  NR chain `rsq / fmul / fmul / fma / fma / fma / fneg / fma / fma`.
  `SqrtE = 0.5 - SqrtH*SqrtS` is subnormal whenever `SqrtR ~ 2^-127`
  (very large `x`), gets flushed to ±0, blocks NR convergence.

The pre-scaling at line 13700 only bounds the small side (`x <
2^-96`); large inputs (e.g. `x > 2^126`) still produce subnormal NR
intermediates.

### Part 2: f64 sqrt
`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:13772-13862`
(`lowerFSQRTF64`).

Same shape: full f64 NR chain (`rsq_f64 / fmul / fmul / fneg / fma /
fma / fma / fneg / fma / fma / fneg / fma / fma`) with no denorm-mode
toggle.  Under `denormal-fp-math="preserve-sign,preserve-sign"`
(legal kernel attr on gfx950, affecting `FP64FP16Denormals`), the
`v_fma_f64` chain runs with FTZ; near-denormal intermediates flush
and Goldschmidt converges to the wrong value.

Mirrors m122 exactly, same kernel attribute, same shape.

### Part 3: missing `setNoFPExcept`
Neither `lowerFSQRTF32` nor `lowerFSQRTF64` sets
`Flags.setNoFPExcept(true)`.  Compare `LowerFDIV32:13342-13343`
which does, with a comment about the chain dependence.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @k(ptr addrspace(1) %out, double %x) #0 {
  %r = call double @llvm.sqrt.f64(double %x)
  store double %r, ptr addrspace(1) %out
  ret void
}
attributes #0 = { "denormal-fp-math"="preserve-sign,preserve-sign" "target-cpu"="gfx950" }
```

Codegen with `clang -O2 -mcpu=gfx950` shows the f64 NR chain (`rsq_f64
+ 7 v_fma_f64`) with **no** `s_setreg hwreg(HW_REG_MODE,4,2)` toggle.
Compare the structurally-identical `LowerFDIV32` asm which inserts
those toggles.

For `x = 2^-1022` (smallest f64 normal), the NR chain's intermediates
land in the subnormal range; under f64 FTZ they flush, and the
result diverges from the IEEE-correct `~2^-511`.

## Why no runtime O0/O2 mismatch

Custom legalization runs at every -O level.  Both -O0 and -O2 emit
the same buggy NR chain.  The FuzzX O0-vs-O2 oracle reports
`any_mismatch=false`.  Witness is SDAG-vs-IR-semantics (or
SDAG-vs-GISel, which uses a different f64 sqrt expansion).

## Suggested fix

Mirror `LowerFDIV32` lines 13364-13416 / 13436-13462 for both
`lowerFSQRTF32` and `lowerFSQRTF64`:

```cpp
// Before NR chain:
if (FPMode.Output != DenormalMode::IEEE) {
  Chain = DAG.getNode(AMDGPUISD::DENORM_MODE, ..., setIEEE, ...);
}

// ... NR FMA chain ...

// After NR chain:
if (FPMode.Output != DenormalMode::IEEE) {
  Chain = DAG.getNode(AMDGPUISD::DENORM_MODE, ..., restorePrev, ...);
}
```

Also set `Flags.setNoFPExcept(true)` on the lowered sqrt result.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces -- no `s_setreg DENORM_MODE` in `lowerFSQRT{F32,F64}` output. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same lowering. |

## Why the fuzzer hasn't caught it

* Default kernel attributes don't set
  `denormal-fp-math="preserve-sign"` for f64.
* The IR fuzzer rarely emits `llvm.sqrt.f64(x)` with `x` in the
  `2^-1022 .. 2^-1015` near-denormal range.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  weight extreme FP constants higher in the f32/f64 constant pool
  and emit kernels with mixed `preserve-sign` / `ieee` denormal
  modes on both f32 and f64.
