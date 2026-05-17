# m024: unsigned division of sign-extended `i16` by `x | 1` returns one

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m024-udiv-or-one/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000000
O0=0x00000001
O2=0x00000000
mismatch=true
```

## Reduction

For work-item zero, the reduced IR computes:

```llvm
%x0 = add i32 %wi, -2
%t = trunc i32 %x0 to i16
%x = sext i16 %t to i32
%den = or i32 %x, 1
%result = udiv i32 %x, %den
```

At `%wi == 0`, `%x` is `0xfffffffe` and `%den` is `0xffffffff`. As unsigned
integers, `%x < %den`, so the quotient is defined and must be zero.

## Root Cause Notes

At `-O0`, ROCm 7.2.3 lowers the unsigned division through a short float
reciprocal sequence:

```asm
v_cvt_f32_u32_e64 v3, v3
v_cvt_f32_u32_e64 v4, v2
v_rcp_f32_e64 v2, v4
v_mul_f32_e64 v2, v3, v2
v_trunc_f32_e64 v2, v2
v_cvt_u32_f32_e64 v2, v2
v_add_u32_e64 v2, v2, v3
v_and_b32_e64 v2, v2, 0x7fff
```

The numerator and denominator are both near `2^32`, so the float conversion is
not precise enough to distinguish them. The generated sequence returns one.
The `-O2` lowering uses a refined unsigned division sequence and returns zero.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000001`, `O2=0x00000000`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: `O0=0x00000001`, `O2=0x00000000`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0x00000001`, `O2=0x00000000`. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses unsigned division when the denominator is
an `or` with a nonzero constant and either directly contains the numerator or
the numerator is a sign-extended `i16` truncation. Set
`FUZZX_ALLOW_M024_UDIV_SEXT_OR=1` to re-enable this shape.
