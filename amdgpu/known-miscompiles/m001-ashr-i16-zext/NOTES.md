# m001: `ashr i16` plus `zext` folded to sign extension

## Summary

An AMDGPU `-O2` compile miscompiles a defined integer expression:

```llvm
%trunc = trunc i32 %x to i16
%shift = ashr i16 %trunc, 8
%zext = zext i16 %shift to i32
%result = xor i32 %x, %zext
```

For `x = 0x7fffffff`, the correct result is `0x7fff0000`. The optimized
AMDGPU code returns `0x80000000`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m001-ashr-i16-zext/reduced.ll
```

The reproducing input is recorded in `reduced.ll` as:

```llvm
; RUN-INPUTS: 0x7fffffff
```

Expected output on the affected toolchains:

```text
input=0x7fffffff
O0=0x7fff0000
O2=0x80000000
mismatch=true
```

## Root Cause

The IR is fully defined: the shift amount is a constant below the `i16` bit
width, and the arithmetic shift result is explicitly zero-extended back to
`i32`.

For `x = 0x7fffffff`:

- `trunc i32 %x to i16` gives `0xffff`.
- `ashr i16 0xffff, 8` gives `0xffff`.
- `zext i16 0xffff to i32` gives `0x0000ffff`.
- `0x7fffffff xor 0x0000ffff` gives `0x7fff0000`.

The optimized AMDGPU output instead contains:

```asm
v_xor_b32_sdwa v2, v2, sext(v2) ... src1_sel:BYTE_1
```

That computes `x ^ sext(byte1(x))`. For this input, `byte1(x)` is `0xff`, so
the folded value becomes `0xffffffff`, producing `0x80000000`. The fold keeps
the arithmetic sign extension of the selected byte but loses the IR's required
zero-extension of the final `i16` value.

The unoptimized code keeps the 16-bit arithmetic shift shape:

```asm
v_ashrrev_i16_e64 ...
v_xor_b32_e64 ...
```

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Reproduces. |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |

Original fuzzer input SHA-1:
`33162d6ff2cc0b53f97e3fe0e8ef87dbafa2dbf8`.

## Fuzzer Suppression

The directed C++ fuzzer suppresses this known shape by default. Set
`FUZZX_ALLOW_M001_ASHR_I16_ZEXT=1` to re-enable it when replaying old fuzzer
inputs.
