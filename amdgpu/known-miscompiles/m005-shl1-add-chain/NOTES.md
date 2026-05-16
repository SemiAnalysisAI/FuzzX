# m005: `-O0` scalarizes a divergent `shl1/add` chain

## Summary

Upstream AMDGPU `-O0` miscompiles a five-step wrapping integer recurrence:

```llvm
%s1 = shl i32 %x0, 1
%x1 = add i32 %s1, 84017408
...
%s5 = shl i32 %x4, 1
%x5 = add i32 %s5, 84017408
```

With two work-items and inputs `0,1`, lane 1 should produce `0x9b3e1f20`.
The affected `-O0` compile returns lane 0's value, `0x9b3e1f00`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m005-shl1-add-chain/reduced.ll
```

The reproducer records the required input vector and LLVM build:

```llvm
; RUN-INPUTS: 0,1
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
[0] input=0x00000000 O0=0x9b3e1f00 O2=0x9b3e1f00 mismatch=false
[1] input=0x00000001 O0=0x9b3e1f00 O2=0x9b3e1f20 mismatch=true
any_mismatch=true
```

## Root Cause

This appears to be the same `-O0` scalarization bug class as
[m003](../m003-shl3-add-chain/NOTES.md), but with a shift-by-1 recurrence.
The IR is fully defined: the shifts are by a constant less than the bit width,
and the additions use ordinary wrapping integer semantics.

The affected `-O0` lowering scalarizes the loaded divergent value through a
first-lane read and then broadcasts the scalar result to all lanes:

```asm
global_load_dword v2, v[2:3], off
v_readfirstlane_b32 s0, v2
s_lshl1_add_u32 s0, s0, ...
...
v_mov_b32_e32 v2, s0
global_store_dword v[0:1], v2, off
```

`-O2` keeps the loaded value in a VGPR and computes each lane independently.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |

Original fuzzer input SHA-1:
`f07da10d989bdc3dec6090ad1bd6219abd6cc17e`.

## Fuzzer Suppression

The directed C++ fuzzer now suppresses the broader known five-step `shl/add`
chain shape by default. Set `FUZZX_ALLOW_M005_SHL_ADD_CHAIN=1` or the older
`FUZZX_ALLOW_M003_SHL3_ADD_CHAIN=1` to re-enable this class when replaying old
fuzzer inputs.
