# m077: `amdgcn.rcp.f32(denormal_C)` constant folder ignores f32 flush of the **input** -- sibling to m075

*Discovery method: code inspection.* Found while auditing the
log/exp2/frexp folds in `AMDGPUInstCombineIntrinsic.cpp` per the
follow-up suggested in m075's NOTES.md.  The log/exp2/frexp folds
turn out to be fine (see "Cases ruled out" below).  But the same
`amdgcn_rcp` fold m075 already pinned down for the output-side flush
also has a separate **input-side** flush bug that m075 does not
exercise.

## The buggy fold

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUInstCombineIntrinsic.cpp:1097-1106`

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

The `TODO` only talks about the **output** denormal flush (m075).  The
**input** denormal flush is also missed: when the f32 input is a
denormal `C`, `v_rcp_f32` on gfx950 (default `PreserveSign` mode)
flushes `C` to `±0` first, then `rcp(±0) = ±Inf`.  The fold computes
`1.0 / C` in full APFloat precision and returns whatever finite normal
value falls out -- the hardware would never produce that.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m077-rcp-constant-denormal-input/reduced.ll
```

Observed:

```text
input=0x00000000
O0=0x7f800000   ; correct -- v_rcp_f32(flush(2^-127)) = rcp(+0) = +Inf
O2=0x7f000000   ; wrong   -- fold returns 1.0/2^-127 = 2^127 (finite normal)
mismatch=true
```

The constant `bitcast (i32 4194304 to float)` is `0x00400000`, i.e. the
denormal `2^-127`.

## Why this is distinct from m075

| Bug   | Constant input                   | APFloat `1/C`           | HW result                          |
| ----- | -------------------------------- | ----------------------- | ---------------------------------- |
| m075  | `0x7f000000` (normal `2^127`)    | `0x00400000` (denormal) | `0x00000000` (output flushed)      |
| m077  | `0x00400000` (denormal `2^-127`) | `0x7f000000` (normal)   | `0x7f800000` (+Inf, input flushed) |

Both share the same root cause -- the fold doesn't consult the kernel's
f32 denormal mode -- but the symptoms are different and a fix for one
will not automatically fix the other.  A correct fold needs to model
both:

1. If `C` is denormal and the input-mode flushes: substitute the
   flushed input (`±0`) and re-compute (so `1/0 -> ±Inf`).
2. If the (post-flush) reciprocal is denormal and the output-mode
   flushes: flush the result to `±0` (m075's case).

## Why the fuzzer doesn't see it

* `amdgcn.rcp` is only called with runtime values by the fuzzer's
  directed emitters; the fold is constant-only.
* Even with constants, the fuzzer would have to pick a denormal
  constant (`|C| < 2^-126`), which it never does.
* The interpreter oracle is skipped for any module containing an
  `amdgcn.*` intrinsic, so the divergence wouldn't be caught even if
  the constant got emitted.

## Cases ruled out in `log` / `exp2` / `frexp_mant` / `frexp_exp`

* `amdgcn_log` / `amdgcn_exp2` (line 1166).  The fold already special-cases
  denormal input at line 1200 (`C->isZero() || (isDenormal && isFloatTy)`)
  and returns the flush-mode answer (`log -> -Inf`, `exp2 -> 1.0`), and
  the TODO at 1209 makes it clear that **normal** inputs are deliberately
  not folded.  So neither the input-side nor the output-side denormal
  flush mismatch can be triggered on f32 today.
* `amdgcn_frexp_mant` / `amdgcn_frexp_exp` (line 1214).  Spot-tested
  with denormal inputs (`0x00400000`, `0x00000001`, `0x007fffff`) and
  HW returns the normalized mantissa in `[0.5, 1.0)` plus the
  extended exponent -- exactly matching APFloat `frexp`.  These
  instructions are denormal-aware regardless of f32 flush mode.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces: `O0=0x7f800000`, `O2=0x7f000000`. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Reproduces: `O0=0x7f800000`, `O2=0x7f000000`. |
