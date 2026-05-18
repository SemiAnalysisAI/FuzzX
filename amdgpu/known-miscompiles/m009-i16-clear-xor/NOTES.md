# m009: `i16` low-16 clear xor miscompiled

## Summary

Upstream AMDGPU `-O0` miscompiles an `i16` analogue of the earlier `i8`
byte-clear xor class:

```llvm
%k = trunc i32 %j to i16
%l = add i16 %k, 0
%m = zext i16 %l to i32
%clear = xor i32 %j, %m
```

For input `0`, the reduced testcase should clear the low 16 bits and return
zero. The affected `-O0` compile returns `0x00000023`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m009-i16-clear-xor/reduced.ll
```

The reproducer records the required input and LLVM build:

```llvm
; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
input=0x00000000
O0=0x00000023
O2=0x00000000
mismatch=true
```

## Root Cause

The IR is fully defined: `ctlz` is called with `is_zero_undef=false`, the
integer operations use wrapping semantics, and no poison-generating flags are
present.

For `x = 0`:

- `%b = ctlz(0, false) = 32`
- `%d = trunc(%b) + 1 = 33`
- `%f = 32 xor 33 = 1`
- `%h = trunc(%f) + 1 = 2`
- `%j = 1 xor 2 = 3`
- `%clear = 3 xor zext(trunc(3) + 0) = 0`

The affected `-O0` lowering combines the narrow operations into a
`v_bitop3_b32` sequence that effectively returns `33 xor 2 = 0x23` instead of
clearing `%j` to zero. This is the same broad SDWA / narrow-xor lowering area as
the previous `i8` clear bugs, but for `i16` values.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`; the patched ROCm HEAD result was rechecked
on 2026-05-18.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |

Original fuzzer input SHA-1:
`366fb58fc40b559615fc7e2183086e5e9a71de2c`.

## Fuzzer Suppression

The directed C++ fuzzer now suppresses generated `i16` identity low-16 clear xor
shapes by default. Set `FUZZX_ALLOW_M009_I16_CLEAR_XOR=1` to re-enable this
class when replaying old fuzzer inputs.
