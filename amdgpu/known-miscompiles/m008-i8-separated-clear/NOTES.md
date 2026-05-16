# m008: separated `i8` byte-clear xor miscompiled

## Summary

Upstream AMDGPU `-O0` miscompiles another `i8` byte-clear xor shape:

```llvm
%lo3 = trunc i32 %tmp to i8
%id = add i8 %lo3, 0
%wide3 = zext i8 %id to i32
%result = xor i32 %tmp, %wide3
```

For input `0`, `%tmp` is `0x48`, so the result must be zero. The affected
`-O0` compile returns `0x00000048`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m008-i8-separated-clear/reduced.ll
```

The reproducer records the required input and LLVM build:

```llvm
; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
input=0x00000000
O0=0x00000048
O2=0x00000000
mismatch=true
```

## Root Cause

This is the same identity byte-clear class as
[m002](../m002-i8-clear-xor/NOTES.md) and
[m006](../m006-i8-xor-clear/NOTES.md), but the fuzzer found it with no-op
`add i32 0` separators between the preceding narrow operations and the final
identity narrow operation.

The IR is fully defined: `ctpop` is total, the integer operations use wrapping
semantics, and no poison-generating flags are present.

For `x = 0`:

- `%pop = ctpop(0) = 0`
- `%v1 = 0 xor zext(trunc(0) xor 72) = 72`
- `%v2 = 72 xor zext(trunc(72) - 72) = 72`
- `%tmp = 72`
- `%result = 72 xor zext(trunc(72) + 0) = 0`

The affected `-O0` lowering fails to clear the low byte and leaves `0x48`.
`-O2` folds the identity byte-clear xor to zero.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |

Original fuzzer input SHA-1:
`d70e1b6346757bf0bfff6bab62dc78210a6e7fe4`.

## Fuzzer Suppression

The directed C++ fuzzer now suppresses all generated `i8` identity byte-clear
xor shapes by default. Set `FUZZX_ALLOW_M008_I8_CLEAR_XOR=1`, or the older
`FUZZX_ALLOW_M002_I8_CLEAR_XOR=1` / `FUZZX_ALLOW_M006_I8_CLEAR_XOR=1`, to
re-enable this class when replaying old fuzzer inputs.
