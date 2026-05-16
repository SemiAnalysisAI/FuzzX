# m003: `-O0` scalarizes a divergent `shl3/add` chain

## Summary

Upstream AMDGPU `-O0` miscompiles a five-step wrapping integer recurrence:

```llvm
%s1 = shl i32 %x0, 3
%x1 = add i32 %s1, 195
...
%s5 = shl i32 %x4, 3
%x5 = add i32 %s5, 195
```

With two work-items and inputs `0,1`, lane 1 should produce `0x000e6d9b`.
The affected `-O0` compile returns lane 0's value, `0x000ded9b`.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m003-shl3-add-chain/reduced.ll
```

The reproducer records the required input vector and LLVM build:

```llvm
; RUN-INPUTS: 0,1
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
[0] input=0x00000000 O0=0x000ded9b O2=0x000ded9b mismatch=false
[1] input=0x00000001 O0=0x000ded9b O2=0x000e6d9b mismatch=true
any_mismatch=true
```

## Root Cause

The IR is fully defined. Left shifts are modulo shifts by a constant less than
the bit width, and the additions are ordinary wrapping integer additions.

The affected `-O0` assembly loads the divergent input into a VGPR, then copies
only the first active lane into an SGPR:

```asm
global_load_dword v2, v[2:3], off
v_readfirstlane_b32 s0, v2
s_lshl3_add_u32 s0, s0, 0xc3
s_lshl3_add_u32 s0, s0, 0xc3
s_lshl3_add_u32 s0, s0, 0xc3
s_lshl3_add_u32 s0, s0, 0xc3
s_lshl3_add_u32 s0, s0, 0xc3
v_mov_b32_e32 v2, s0
global_store_dword v[0:1], v2, off
```

That broadcasts lane 0's loaded value to every active lane. `-O2` keeps the
loaded value in a VGPR and emits a vector `v_lshl_add_u32`.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |

Original fuzzer input SHA-1:
`099cc15e52c8ccea6e1c3011ead6b335809d0519`.

## Fuzzer Suppression

The directed C++ fuzzer suppresses chains with five `shl i32 ..., 3` plus
`add i32` pairs by default. Set `FUZZX_ALLOW_M003_SHL3_ADD_CHAIN=1` to re-enable
this shape when replaying old fuzzer inputs.
