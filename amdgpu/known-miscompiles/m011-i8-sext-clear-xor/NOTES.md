# m011: signed `i8` masked clear xor miscompiled

## Summary

Upstream AMDGPU `-O0` miscompiles a signed `i8` identity feeding an xor after
two narrow masks:

```llvm
%e = and i32 %x, 1
%masked = and i32 %e, 2
%lo8 = trunc i32 %masked to i8
%id = add i8 %lo8, 0
%wide8 = sext i8 %id to i32
%result = xor i32 %masked, %wide8
```

For input `2`, `%masked` is zero, so `%result` must also be zero. The affected
`-O0` compile returns `2`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m011-i8-sext-clear-xor/reduced.ll
```

The reproducer records the required input and LLVM build:

```llvm
; RUN-INPUTS: 2
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
input=0x00000002
O0=0x00000002
O2=0x00000000
mismatch=true
```

## Root Cause

The IR is fully defined: all integer operations use wrapping semantics, the
shift-free masks cannot create poison, and there are no `undef`, `nuw`, `nsw`,
`exact`, or `inbounds` operations.

For `x = 2`:

- `%e = 2 & 1 = 0`
- `%masked = 0 & 2 = 0`
- `%wide8 = sext(trunc(0) + 0) = 0`
- `%result = 0 xor 0 = 0`

The affected `-O0` lowering combines the mask and signed narrow xor into a
single `v_bitop3_b32` sequence that returns the original input bit instead of
zero:

```text
s_mov_b32 s2, 2
s_mov_b32 s3, 1
v_mov_b32_e32 v3, s3
v_bitop3_b32 v2, v2, s2, v3 bitop3:0x70
```

This is adjacent to the earlier byte-clear xor bugs, but the reduced testcase
requires a sign-extending `i8` identity. Changing the final extension to `zext`
does not reproduce this reduced case.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |

Original fuzzer input SHA-1:
`f3ae76a3ea7d37d903352cdfc040d4166355815d`.

## Fuzzer Suppression

The directed C++ fuzzer now suppresses generated `i8` sign-extended identity
clear xor shapes by default. Set `FUZZX_ALLOW_M011_I8_SEXT_CLEAR_XOR=1` to
re-enable this class when replaying old fuzzer inputs.
