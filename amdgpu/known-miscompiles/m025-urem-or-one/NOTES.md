# m025: unsigned remainder of sign-extended `i16` by `x | 1` is masked

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m025-urem-or-one/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000000
O0=0x00007fff
O2=0xfffffffe
mismatch=true
```

## Reduction

For work-item zero, the reduced IR computes:

```llvm
%x0 = add i32 %wi, -2
%t = trunc i32 %x0 to i16
%x = sext i16 %t to i32
%den = or i32 %x, 1
%result = urem i32 %x, %den
```

At `%wi == 0`, `%x` is `0xfffffffe` and `%den` is `0xffffffff`. As unsigned
integers, `%x < %den`, so the remainder is the numerator, `0xfffffffe`.

## Root Cause Notes

At `-O0`, ROCm 7.2.3 uses the same short float reciprocal quotient estimate as
m024 and then computes the remainder from that quotient:

```asm
v_cvt_f32_u32_e64 v5, v3
v_cvt_f32_u32_e64 v6, v4
v_rcp_f32_e64 v3, v6
v_mul_f32_e64 v3, v5, v3
v_trunc_f32_e64 v3, v3
v_cvt_u32_f32_e64 v3, v3
v_add_u32_e64 v3, v3, v5
v_mul_lo_u32 v3, v3, v4
v_sub_u32_e64 v2, v2, v3
v_and_b32_e64 v2, v2, 0x7fff
```

The quotient estimate is wrong for values near `2^32`, and the final mask
leaves `0x7fff` instead of the defined remainder. The `-O2` lowering uses a
refined unsigned division sequence and returns `0xfffffffe`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00007fff`, `O2=0xfffffffe`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: `O0=0x00007fff`, `O2=0xfffffffe`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0x00007fff`, `O2=0xfffffffe`. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses unsigned remainder when the denominator is
an `or` with a nonzero constant. Set `FUZZX_ALLOW_M025_UREM_SEXT_OR=1` to
re-enable this shape.
