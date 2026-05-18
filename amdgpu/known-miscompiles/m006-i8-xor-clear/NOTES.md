# m006: `i8 xor` feeding byte-clear xor miscompiled

## Summary

Upstream AMDGPU `-O0` miscompiles another adjacent `i8` narrow operation feeding
a byte-clear xor:

```llvm
%mixed_lo = trunc i32 %mixed to i8
%mixed_lo_wide = zext i8 %mixed_lo to i32
%result = xor i32 %mixed, %mixed_lo_wide
```

For input `0`, `%mixed` is `0x2d`, so the result must be zero. The affected
`-O0` compile returns `0x0000000d`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m006-i8-xor-clear/reduced.ll
```

The reproducer records the required input and LLVM build:

```llvm
; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
input=0x00000000
O0=0x0000000d
O2=0x00000000
mismatch=true
```

## Root Cause

This appears to be the same `v_bitop3_b32` byte-clear lowering bug class as
[m002](../m002-i8-clear-xor/NOTES.md), but with a preceding `i8 xor` instead of
a preceding `i8 add`.

The IR is fully defined. `ctlz` is called with `is_zero_undef = false`, and all
integer operations have wrapping semantics without poison-generating flags.

For `x = 0`:

- `%lz = ctlz(0) = 32`
- `%xlo = trunc(32) xor 45 = 13`
- `%mixed = 32 xor 13 = 45`
- `%result = 45 xor zext(trunc(45)) = 0`

The affected `-O0` code lowers the final byte-clear xor to:

```asm
v_xor_b32_e64 v3, v3, 45
v_bitop3_b32 v2, v2, v3, 0xff bitop3:0x88
```

That returns `0x0d` instead of clearing the low byte to zero. `-O2` folds the
expression to a zero store.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`; the patched ROCm HEAD result was rechecked
on 2026-05-18.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |

Original fuzzer input SHA-1:
`dc6b9f71a3e2d0d71928b817b71145bd049e8583`.

## Fuzzer Suppression

The directed C++ fuzzer now suppresses adjacent `i8` narrow operations followed
by an identity byte-clear xor by default. Set `FUZZX_ALLOW_M006_I8_CLEAR_XOR=1`
or the older `FUZZX_ALLOW_M002_I8_CLEAR_XOR=1` to re-enable this class when
replaying old fuzzer inputs.
