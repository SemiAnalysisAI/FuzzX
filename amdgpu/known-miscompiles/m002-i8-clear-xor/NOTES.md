# m002: `i8` clear-low-byte xor lowered with the wrong truth table

## Summary

Upstream AMDGPU `-O0` miscompiles a defined expression that xors a value with
the zero-extended low byte of itself:

```llvm
%mixed_lo = trunc i32 %mixed to i8
%mixed_lo_wide = zext i8 %mixed_lo to i32
%result = xor i32 %mixed, %mixed_lo_wide
```

For the reduced input `0`, `%mixed` is `0x3f`, so the correct result is `0`.
The affected `-O0` compile returns `0x20`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m002-i8-clear-xor/reduced.ll
```

The reproducer records both the input and the LLVM build it was found against:

```llvm
; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
input=0x00000000
O0=0x00000020
O2=0x00000000
mismatch=true
```

## Root Cause

The IR is fully defined. The `ctlz` input is forced nonzero with `or i32 %x, 1`,
the `ctlz` `is_zero_undef` argument is `false`, and all integer operations use
normal wrapping semantics without poison-generating flags.

For `x = 0`, the reduced program computes:

- `%nonzero = 1`
- `%lz = ctlz(1) = 31`
- `%plus = trunc(31) + 1 = 32`
- `%wide = 32`
- `%mixed = 31 xor 32 = 63`
- `%result = 63 xor zext(trunc(63)) = 0`

The affected `-O0` code lowers the final byte-clear xor to:

```asm
v_add_u16_e64 v3, v3, 1
v_bitop3_b32 v2, v2, v3, 0xff bitop3:0x88
```

That sequence returns `0x20` for the reduced input instead of clearing the low
byte to zero. `-O2` folds the expression to a constant zero store.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |

Original fuzzer input SHA-1:
`b9206f5c71a6054245bac70ba0e87381e84fd736`.

## Fuzzer Suppression

The directed C++ fuzzer suppresses this known adjacent `i8` narrow/xor shape by
default. Set `FUZZX_ALLOW_M002_I8_CLEAR_XOR=1` to re-enable it when replaying
old fuzzer inputs.
