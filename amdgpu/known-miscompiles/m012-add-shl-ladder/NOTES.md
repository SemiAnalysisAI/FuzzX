# m012: add/shl ladder scalarized through lane 0

## Summary

Upstream AMDGPU `-O0` miscompiles a divergent five-step `add`/`shl` ladder:

```llvm
%a0 = add i32 %x, 1
%s0 = shl i32 %a0, 1
%a1 = add i32 %s0, 1
%s1 = shl i32 %a1, 1
%a2 = add i32 %s1, 1
%s2 = shl i32 %a2, 1
%a3 = add i32 %s2, 1
%s3 = shl i32 %a3, 1
%a4 = add i32 %s3, 1
%s4 = shl i32 %a4, 1
%result = add i32 %s4, 1
```

For inputs `[0, 1]`, lane 0 should return `0x3f` and lane 1 should return
`0x5f`. The affected `-O0` compile returns `0x3f` for both lanes.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m012-add-shl-ladder/reduced.ll
```

The reproducer records the required inputs and LLVM build:

```llvm
; RUN-INPUTS: 0, 1
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

Expected output on the affected toolchain:

```text
[0] input=0x00000000 O0=0x0000003f O2=0x0000003f mismatch=false
[1] input=0x00000001 O0=0x0000003f O2=0x0000005f mismatch=true
any_mismatch=true
```

## Root Cause

The IR is fully defined. All integer arithmetic uses wrapping semantics, all
shift amounts are constant and below the `i32` bit width, and there are no
`undef`, `poison`, `nuw`, `nsw`, `exact`, or `inbounds` operations.

For lane 0, the recurrence `v = (v + 1) << 1` repeated five times, followed by
`+ 1`, returns `63`. For lane 1, it returns `95`.

The affected `-O0` lowering scalarizes the divergent value after the first add:

```text
v_add_u32_e64 v2, v2, s1
v_readfirstlane_b32 s0, v2
s_lshl1_add_u32 s0, s0, s1
s_lshl1_add_u32 s0, s0, s1
s_lshl1_add_u32 s0, s0, s1
s_lshl1_add_u32 s0, s0, s1
s_lshl1_add_u32 s0, s0, s1
v_mov_b32_e32 v2, s0
```

That sequence computes the ladder from lane 0's value and broadcasts it back to
all active lanes. This is the same broad scalarization family as m003 and m005,
but the reduced shape is an add-before-shift ladder rather than the earlier
shift-before-add forms.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |

Original fuzzer input SHA-1:
`57c3e07baa4561ecfef2cdefcd6c6627299120cd`.

## Fuzzer Suppression

The directed C++ fuzzer now suppresses generated add/shl ladder shapes before
they reach this scalarization-prone form. Set
`FUZZX_ALLOW_M012_ADD_SHL_LADDER=1` to re-enable this class when replaying old
fuzzer inputs.
