# m013: private-memory fshl stack values are intermittent at -O0

## Summary

AMDGPU `-O0` intermittently misexecutes a fully defined kernel containing five
fixed-size private-memory allocas feeding dynamic `llvm.fshl.i32` operations.
The same IR compiled at `-O2` is stable.

The reduced testcase stores a value in slot 0 of a two-element private array,
stores zero in adjacent slot 1, reloads slot 0, applies `fshl`, and feeds
`ctpop` into the next copy of the same pattern. Five copies are needed in the
reduced case.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m013-private-memory-fshl/reduced.ll
```

The reproducer records the required input shape and repeat count:

```llvm
; RUN-INPUTS: 0x0*129
; RUN-REPEAT: 100
; RUN-LLVM-BUILD: build/llvm-fuzzer
```

The bug is intermittent, so the exact iteration and lane may vary. One observed
run on the affected toolchain:

```text
iteration=1
index=128
input=0x00000000
O0=0x6ccf7895
O2=0x5b37de25
mismatch=true
```

## Root Cause

The IR is fully defined. All private-memory loads read initialized stores, all
GEPs use in-range constant indices without `inbounds`, the funnel-shift amount
is masked to `[0, 31]`, and there are no `undef`, `poison`, `nuw`, `nsw`,
`exact`, or division operations.

The affected `-O0` lowering keeps the fixed allocas in scratch memory and emits
a dynamic stack sequence. In the reduced case the O0 assembly has
`.amdhsa_uses_dynamic_stack 1`, `.amdhsa_private_segment_fixed_size 16`, and
repeated scalar stack-pointer bumps by `0x400` before `scratch_store_dword` /
`scratch_load_dword` pairs:

```text
s_mov_b32 s7, 0x400
s_add_i32 s3, s2, s7
s_mov_b32 s32, s3
scratch_store_dword off, v2, s2
s_add_i32 s4, s2, 4
scratch_store_dword off, v2, s4
scratch_load_dword v3, off, s2
```

By contrast, `-O2` proves the private-memory round trips redundant and emits no
scratch usage (`.amdhsa_private_segment_fixed_size 0`). The intermittent O0
result is consistent with the O0 scratch/dynamic-stack lowering reading an
unstable value for one of the reloaded private slots.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Reproduces. |

Original fuzzer input SHA-1:
`cfe35195227876745019ca2b22997494b9edd99c`.

Original fuzzer input bytes:

```text
ad 12 ef 8a
```

## Fuzzer Suppression

The directed C++ fuzzer now caps generated private-memory ops at two by
default. Set `FUZZX_ALLOW_M013_PRIVATE_MEMORY_FSHL=1` to re-enable the
three-or-more private-memory/funnel-shift shape when replaying this fuzzer
input.

A later structured-CFG variant with original fuzzer input SHA-1
`ea7a9d7c7d69ca16f1a670c154a71ed4f71f6e56` showed that three private-memory
ops are enough to hit the same scratch-stack issue, so the suppression is
intentionally broader than the five-copy linear reduced testcase.
