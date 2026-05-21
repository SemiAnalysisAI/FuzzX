# m137: `LowerF64ToF16Safe` discards NaN payload bits; `afn` flag flip changes NaN output

*Discovery method: code inspection.*  Sibling shape to m133
(`getCanonicalConstantFP` drops NaN payload while HW preserves it).
Same family: AMDGPU custom-lowering path canonicalises NaN payload
where the HW direct path would carry the bits through.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:3787-3873`
(`AMDGPUTargetLowering::LowerF64ToF16Safe`), used via
`SIISelLowering.cpp:8599` when `FP_ROUND f64 -> f16` is custom-lowered
on gfx950.

For `E == 1039` (the biased exponent of f64 NaN/Inf), the code
collapses the result mantissa to:

```cpp
SDValue I = DAG.getNode(ISD::OR, DL, MVT::i32,
    DAG.getSelectCC(DL, M, Zero, DAG.getConstant(0x0200, DL, MVT::i32),
                    Zero, ISD::SETNE),
    DAG.getConstant(0x7c00, DL, MVT::i32));
...
V = DAG.getSelectCC(DL, E, DAG.getConstant(1039, DL, MVT::i32),
                    I, V, ISD::SETEQ);
```

Every f64 NaN therefore reduces to `0x7e00 | sign` regardless of the
input payload.  The QNaN bit (top mantissa bit) is forced and **all
lower payload bits are erased**.  sNaN is silently quieted with no
preservation of the sNaN-distinguishing bit pattern.

Compare to the HW chain f64 -> f32 -> f16
(`v_cvt_f32_f64` + `v_cvt_f16_f32`), which preserves the top of the
NaN payload through the two conversions.

## Reproducer

`reduced.ll` builds an f64 NaN with payload `0x1bcde_abcd1234` and
stores **both** lowering paths' results side-by-side in one i32 (low
half via `fptrunc double -> half` direct; high half via the explicit
`double -> float -> half` chain):

```llvm
; input: f64 bits = 0x7ff1bcde_abcd1234 (qNaN with payload)
%r_dir = fptrunc double %xd_dir to half          ; LowerF64ToF16Safe
%xf    = fptrunc double %xd_via to float         ; v_cvt_f32_f64
%r_via = fptrunc float  %xf     to half          ; v_cvt_f16_f32
%combined = (zext(r_dir) << 16) | zext(r_via)
```

`run_ll_reproducer.sh` output:

```
input(lo32=0xabcd1234, hi32=0x7ff1bcde)
direct fptrunc f64->f16 (LowerF64ToF16Safe) -> 0x7e00    ; payload dropped
via    fptrunc f64->f32->f16 (HW v_cvt chain) -> 0x7e6f  ; payload partly preserved
```

The same kernel produces two different NaN payloads for the same input,
which makes the bug observable even within a single LLVM module.

With the `afn` fast-math flag on the `fptrunc`, the lowering takes the
f32 detour and yields `0x7e6f`; without `afn`, it takes
`LowerF64ToF16Safe` and yields `0x7e00`.  LangRef `afn` ("approximate
functions") permits imprecise approximation but does NOT license
changing NaN payload preservation.

## Why this matters

* While IEEE-754 permits payloads to vary across implementations, this
  is a *target-internal* asymmetry: the AMDGPU lowering of `FP_ROUND
  f64->f16` and the HW chain disagree, so the user observes different
  NaN payloads from semantically-equivalent IR depending on opt level
  or fast-math flag.
* Identical pattern to m133 (`getCanonicalConstantFP` drops payload
  where HW preserves it).  Both are AMDGPU "Safe expansion" paths
  taking a position that contradicts the HW direct path.

## Suggested fix

In `LowerF64ToF16Safe`, when `E == 1039 && M != 0` (NaN), forward the
top 9 bits of the f64 significand into the f16 mantissa instead of
forcing `0x200`.  Concretely:

```cpp
// For NaN: f16 mantissa = top 9 bits of f64 mantissa, with quiet bit set.
SDValue NaNMant = DAG.getNode(ISD::SRL, DL, MVT::i32, M_lo32,
                              DAG.getConstant(42, DL, MVT::i32));  // f64 mant 52 -> f16 mant 10
NaNMant = DAG.getNode(ISD::OR, DL, MVT::i32, NaNMant,
                      DAG.getConstant(0x0200, DL, MVT::i32));     // ensure quiet
SDValue I = DAG.getNode(ISD::OR, DL, MVT::i32, NaNMant,
                        DAG.getConstant(0x7c00, DL, MVT::i32));
```

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (direct=0x7e00, via=0x7e6f). |
| ROCm 7.1.1 | Same defect (LowerF64ToF16Safe code path unchanged). |

## Why the fuzzer hasn't caught it

* The O0-vs-O2 oracle agrees (both pipelines pick the same lowering
  for a given flag).  Needs a same-IR `afn`-vs-strict differential or
  an interpreter oracle for NaN payloads.
* Per `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should mix `fptrunc double -> half` with and without `afn` on the
  same NaN-valued operand to surface this.
