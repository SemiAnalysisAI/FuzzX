# m105: `fptosi.sat` / `fptoui.sat` from `bfloat` to `i64` silently clamps at i32 range

*Discovery method: code inspection of AMDGPU SDAG fp-to-int lowering.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:3979-3986`
(`AMDGPUTargetLowering::LowerFP_TO_INT_SAT`):

```cpp
// Saturate at i32 for i64 dst and f16/bf16 src (will invoke f16 promotion
// below)
if (DstVT == MVT::i64 &&
    (SrcVT == MVT::f16 || SrcVT == MVT::bf16 ||
     (SrcVT == MVT::f32 && Src.getOpcode() == ISD::FP16_TO_FP))) {
  const SDValue Int32VTOp = DAG.getValueType(MVT::i32);
  return DAG.getNode(OpOpcode, DL, DstVT, Src, Int32VTOp);
}
```

The shortcut "saturate at i32, sign/zero-extend to i64" is sound for `f16`
(max finite `65504` fits in `i32`) and for the `FP16_TO_FP` decay case
(its source bound by f16's range too).  It is **wrong for `bf16`**: bf16
shares f32's 8-bit exponent (max finite ~`3.39e38`), so values in
`[INT32_MAX+1, INT64_MAX]` silently clamp to `INT32_MAX` instead of
returning the true integer (or `INT64_MAX`, for genuine overflow).

The symmetric `fptoui.sat` case has the same defect.  The NaN handling
stays correct (`V_CVT_I32_F32` returns `0` on NaN, matching
`fptosi.sat` semantics).

## Reproducer

`reduced.ll`:

```llvm
declare i64 @llvm.fptosi.sat.i64.bf16(bfloat)
define amdgpu_kernel void @k(ptr addrspace(1) %out, bfloat %x) {
  %r = call i64 @llvm.fptosi.sat.i64.bf16(bfloat %x)
  store i64 %r, ptr addrspace(1) %out
  ret void
}
```

Test value: bf16 `0x4f80` = `2.0**32` = `4294967296`.

* Expected (IR `fptosi.sat` semantics): `0x0000000100000000`.
* Observed (SDAG `-O0` & `-O2` on gfx950): `0x000000007fffffff` (`INT32_MAX`,
  sign-extended).

Generated code (gfx950, llc, bf16 input in `s2`):

```asm
s_lshl_b32 s2, s2, 16          ; bf16 << 16 = f32 bit pattern
v_cvt_i32_f32_e64 v1, s2
s_mov_b32 s2, 0x7fffffff
v_min_i32_e64 v1, v1, s2       ; <-- clamps at INT32_MAX
s_mov_b32 s2, 0x80000000
v_max_i32_e64 v2, v1, s2
v_ashrrev_i32_e64 v1, 31, v2   ; sext to i64
```

## Why no O0/O2 mismatch in the FuzzX harness

The bug is in **Custom legalization** (not in a combine), so the buggy
shortcut runs at every optimisation level.  Both `-O0` and `-O2` produce
identical wrong code -- the FuzzX O0-vs-O2 differential oracle does NOT
flag this.

The witness is `SDAG vs IR semantics`.  A correct reference compile (host
emulation of `fptosi.sat`, or GISel which routes bf16→i64 differently) is
needed to flag the divergence.

## Suggested fix

Drop `bf16` from the i32 shortcut and route it through a dedicated bf16→i64
saturating expansion, which is structurally `f32→i64` (since the bf16→f32
promotion is free) with proper clamp at `INT64_MIN`/`INT64_MAX`:

```cpp
if (DstVT == MVT::i64 &&
    (SrcVT == MVT::f16 ||
     (SrcVT == MVT::f32 && Src.getOpcode() == ISD::FP16_TO_FP))) {
  const SDValue Int32VTOp = DAG.getValueType(MVT::i32);
  return DAG.getNode(OpOpcode, DL, DstVT, Src, Int32VTOp);
}
```

For bf16, promote to f32, run the existing `LowerFP_TO_INT64` saturating
path with the `i64` bounds.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces: stores `0x000000007fffffff` for bf16 input `0x4f80`. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same buggy shortcut. |

Not a HEAD-only regression.  The shortcut has been in tree since the
bf16 lowering was added.

## Why the fuzzer doesn't see it

* The current AMDGPU IR fuzzer doesn't emit `llvm.fptosi.sat.i64.bf16` /
  `llvm.fptoui.sat.i64.bf16` with bf16 inputs.
* The O0-vs-O2 oracle is blind to constant Custom-legalization bugs (both
  pipelines hit the same Custom path).
* Per `MEMORY.md` (Prefer-random-over-idioms), the fix is to let the
  random emitter pick bf16 source for the `fp_to_int_sat` family,
  weighted to include the over-`2**31` range.
