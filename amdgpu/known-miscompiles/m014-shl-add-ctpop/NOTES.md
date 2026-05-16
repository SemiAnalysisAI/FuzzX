# m014: shl/add chain feeding ctpop scalarized through lane 0

## Summary

Upstream AMDGPU `-O0` miscompiles a divergent four-step `shl`/`add` chain when
the result feeds `llvm.ctpop.i32`:

```llvm
%t0 = shl i32 %x, 1
%t1 = add i32 %t0, 1
%t2 = shl i32 %t1, 1
%t3 = add i32 %t2, 1
%t4 = shl i32 %t3, 1
%t5 = add i32 %t4, 1
%t6 = shl i32 %t5, 1
%t7 = add i32 %t6, 1
%result = call i32 @llvm.ctpop.i32(i32 %t7)
```

For inputs `[0, 1]`, lane 0 should return `4` and lane 1 should return `5`.
The affected `-O0` compile returns `4` for both lanes.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m014-shl-add-ctpop/reduced.ll
```

Expected output on the affected toolchain:

```text
[0] input=0x00000000 O0=0x00000004 O2=0x00000004 mismatch=false
[1] input=0x00000001 O0=0x00000004 O2=0x00000005 mismatch=true
any_mismatch=true
```

## Root Cause

The IR is fully defined. All arithmetic uses wrapping semantics, all shifts use
constant in-range amounts, and there are no `undef`, `poison`, `nuw`, `nsw`,
`exact`, `inbounds`, or division operations.

The affected `-O0` lowering reads only one divergent lane before evaluating the
chain:

```text
v_readfirstlane_b32 s2, v2
s_lshl1_add_u32 s2, s2, s3
s_lshl1_add_u32 s2, s2, s3
s_lshl1_add_u32 s2, s2, s3
s_lshl1_add_u32 s2, s2, s3
v_bcnt_u32_b32 v2, s2, 0
```

That computes `ctpop((((lane0 << 1) + 1) ...))` and broadcasts the result to all
active lanes. This is the same broad scalarization family as m003, m005, and
m012, but the four-step chain only miscompiled once its result fed `ctpop`.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| Upstream LLVM 23.0.0git, commit `a1403139d0ba7fdfc82d6ae8a2884f27fec9fa15`, built with sanitizer coverage | Reproduces. |
| ROCm 7.1.1 clang 20.0.0git, commit `27682a16360e33e37c4f3cc6adf9a620733f8fe1` | Does not reproduce this reduced case. |

Original fuzzer input SHA-1 values:

```text
1d3be17a5edcc02ab532c9bc7349a9216d8795ba
622126ebaa826b2b978babd12eb70a5b9de8d0c5
```

## Fuzzer Suppression

The directed C++ fuzzer now suppresses four-step `shl/add` chains immediately
before `ctpop`. Set `FUZZX_ALLOW_M014_SHL_ADD_CTPOP=1` to re-enable this class
when replaying old fuzzer inputs.
