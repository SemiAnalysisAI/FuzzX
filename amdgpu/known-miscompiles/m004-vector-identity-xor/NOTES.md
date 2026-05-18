# m004: vector lane-0 identity lost before xor

## Summary

Upstream AMDGPU `-O0` miscompiles a defined vector-lane identity. The reduced
program builds a `<2 x i32>` vector whose lane 0 is `%y2`, subtracts a zero
vector, extracts lane 0, and xors it with `%y2`:

```llvm
%sub = sub <2 x i32> %w1, zeroinitializer
%e = extractelement <2 x i32> %sub, i32 0
%result = xor i32 %y2, %e
```

Since `%e` is `%y2`, the result must be zero. The affected `-O0` compile returns
`0xfffffffd` for input `0`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m004-vector-identity-xor/reduced.ll
```

The reproducer records the required input and LLVM build:

```llvm
; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
input=0x00000000
O0=0xfffffffd
O2=0x00000000
mismatch=true
```

## Root Cause

The IR is fully defined: the vector operations are fixed-width integer add,
mul, sub, xor, insert, and extract operations with no poison-generating flags.

For `x = 0`, the reduced program computes:

- `%mul` lane 1 is `1 * -1 = 0xffffffff`, so `%y1 = 0xffffffff`.
- `%addv` lane 1 is `1 + 1 = 2`, so `%y2 = 0xffffffff xor 2 = 0xfffffffd`.
- The final vector `sub` extracts lane 0, which is still `%y2`.
- `%y2 xor %y2` is `0`.

The affected `-O0` selection drops that final lane-0 identity. The generated
code computes and stores `%y2` instead of xoring it with the extracted lane:

```asm
v_xad_u32 v6, v2, v4, 1
v_bitop3_b32 v2, v2, v3, v4 bitop3:0x66
global_store_dword v[0:1], v2, off
```

`-O2` folds the final identity xor to a zero store.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`; the patched ROCm HEAD result was rechecked
on 2026-05-18.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |

Original fuzzer input SHA-1:
`c045b784b4e306fa7ddff1c53e14928051a62f64`.

## Fuzzer Suppression

The directed C++ fuzzer suppresses the known vector lane-0 identity xor after
two prior vector operations by default. Set
`FUZZX_ALLOW_M004_VECTOR_IDENTITY_XOR=1` to re-enable this shape when replaying
old fuzzer inputs.
