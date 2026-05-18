# m010: `i16` sign-extended clear xor miscompiled

## Summary

Upstream AMDGPU `-O0` miscompiles a sign-extended `i16` identity feeding an
xor:

```llvm
%lo16 = trunc i32 %v to i16
%id = ashr i16 %lo16, 0
%wide16 = sext i16 %id to i32
%result = xor i32 %v, %wide16
```

For input `0`, `%v` is `63`, so `%wide16` is also `63` and the result must be
zero. The affected `-O0` compile returns `0x0000001f`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m010-i16-sext-clear-xor/reduced.ll
```

The reproducer records the required input and LLVM build:

```llvm
; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
input=0x00000000
O0=0x0000001f
O2=0x00000000
mismatch=true
```

## Root Cause

The IR is fully defined: `ctlz` is called with `is_zero_undef=false`, the
integer operations use wrapping semantics, the shift amount is zero, and no
poison-generating flags are present.

For `x = 0`:

- `%clz = ctlz(0, false) = 32`
- `%dec = trunc(32) - 1 = 31`
- `%v = 32 xor zext(31) = 63`
- `%wide16 = sext(trunc(63) >> 0) = 63`
- `%result = 63 xor 63 = 0`

The affected `-O0` lowering combines the narrow operations into a
`v_bitop3_b32` sequence using the pre-clear values and returns `31` instead of
zero. The reduced `-O0` object contains:

```text
v_add_u16_e64 v3, v3, s2
v_bitop3_b32 v2, v2, v3, s2 bitop3:0x88
```

This is adjacent to the previous narrow clear bugs, but it specifically needs a
sign-extending `i16` identity.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`; the patched ROCm HEAD result was rechecked
on 2026-05-18.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |

Original fuzzer input SHA-1 values in this cluster:

- `7a6a14f478859ce0e3f7356ff2be8bcf26694a88`
- `b177e95bd8577f894d42ff1a89023f6e6ed635d4`
- `692dece1852a70727b8cbad71ddc17398c9d34f4`
- `537f4837db5e00059ad74705eff858ebf57294c9`

## Fuzzer Suppression

The directed C++ fuzzer now suppresses generated `i16` sign-extended identity
clear xor shapes by default. Set `FUZZX_ALLOW_M010_I16_SEXT_CLEAR_XOR=1` to
re-enable this class when replaying old fuzzer inputs.
