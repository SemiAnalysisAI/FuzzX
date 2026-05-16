# m007: vector shift-by-zero identity lost before xor

## Summary

Upstream AMDGPU `-O0` miscompiles a vector lane-0 identity where a vector shift
by zero should preserve lane 0:

```llvm
%vec = shl <2 x i32> %v1, zeroinitializer
%e = extractelement <2 x i32> %vec, i32 0
%result = xor i32 %mixed, %e
```

Since `%e` is `%mixed`, the result must be zero. The reduced affected `-O0`
compile returns `0xabababcc`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m007-vector-shl-identity-xor/reduced.ll
```

The reproducer records the required input and LLVM build:

```llvm
; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
input=0x00000000
O0=0xabababcc
O2=0x00000000
mismatch=true
```

## Root Cause

This is the same vector lane-0 identity bug class as
[m004](../m004-vector-identity-xor/NOTES.md), but using `shl <2 x i32>` by a
zero vector rather than `sub` by zero.

The IR is fully defined: the scalar `i8` shift amount is a constant below the
bit width, the vector shift amount is zero in every lane, and there are no
poison-generating arithmetic flags.

For `x = 0`:

- `%a = 0xabababac`
- `trunc(%a) << 3` in `i8` gives `0x60`
- `%mixed = 0xabababac xor 0x60 = 0xabababcc`
- the vector shift by zero leaves lane 0 as `%mixed`
- `%mixed xor %mixed` is `0`

The affected `-O0` lowering folds the vector identity into a `v_bitop3_b32`
sequence and stores `%mixed` instead of zero:

```asm
v_lshlrev_b16_e64 v3, 3, v3
v_bitop3_b32 v2, v2, v3, 0xabababac bitop3:0x66
global_store_dword v[0:1], v2, off
```

`-O2` folds the identity xor to a zero store.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |

Original fuzzer input SHA-1:
`cd5df35736aa0812c2e76e0e20203f8b3f4afb40`.

## Fuzzer Suppression

The directed C++ fuzzer now suppresses vector lane-0 identity xor shapes by
default. Set `FUZZX_ALLOW_M007_VECTOR_IDENTITY_XOR=1` or the older
`FUZZX_ALLOW_M004_VECTOR_IDENTITY_XOR=1` to re-enable this class when replaying
old fuzzer inputs.
