# m053: byte-dot high-bit test regresses in `-O0`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=1
input=0x1
o0=0x80000000
o2=0x0
expected=0x0
```

The standalone reproducer keeps the byte-dot/high-bit shape and flips the final
high bit so lane 1 preserves the original observed output values:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m053-bytedot-highbit/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[0] input=0x00000000 O0=0x80000000 O2=0x80000000 mismatch=false
[1] input=0x00000001 O0=0x80000000 O2=0x00000000 mismatch=true
any_mismatch=true
```

ROCm 7.2.3 passes this reduced testcase. LLVM HEAD and ROCm HEAD reproduce the
mismatch.

## Reduction

For lane 1, the reduced program computes a mixed value from `%input ^
workitem.id.x * 0x9e3779b9`, extracts byte fields, forms a blend, and then
builds a byte-dot-style accumulator:

```llvm
%mul0 = mul i32 %a0, %b0
%acc0 = sub i32 150, %mul0
...
%acc3 = xor i32 %acc2, %mul3.shift
%packed = or i32 %or2, %p3
%result = add i32 %acc3, %packed
%high = and i32 %result, -2147483648
%flipped = xor i32 %high, -2147483648
```

Straight-line CPU arithmetic gives `%result == 0xffffba56`, so `%high` is
`0x80000000` and `%flipped` is `0x00000000`. LLVM HEAD `-O0` instead computes
`%high == 0`, making the final flipped result `0x80000000`.

## Root Cause Notes

The ROCm 7.2.3 `-O0` lowering keeps the byte blend in a form that preserves the
lane-1 bytes:

```asm
v_xor_b32_e64 v1, v1, v5
v_xor_b32_e64 v3, v1, 0xffffff00
v_bitop3_b32 v2, v2, v3, v4 bitop3:0x48
v_bitop3_b32 v4, v1, v2, s4 bitop3:0xce
...
v_xad_u32 v1, v1, v2, v3
```

LLVM HEAD `-O0` rewrites the same region through a different `v_bitop3_b32` /
`v_bfi_b32` sequence:

```asm
v_bitop3_b32 v2, v2, s3, v3 bitop3:0x6c
v_xor_b32_e64 v2, v2, v6
v_bfi_b32 v3, v2, s2, v3
...
v_xad_u32 v2, v2, v3, v4
```

That rewrite changes the byte value feeding the final packed add, clearing the
high bit that should be set before the final xor. The `-O2` lowering recognizes
the byte-dot-like expression and produces the CPU/oracle value.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: lane 1 `O0=0x00000000`, `O2=0x00000000`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0x80000000`, `O2=0x00000000`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0x80000000`, `O2=0x00000000`. |

## Fuzzer Follow-Up

The fuzzer now rejects byte-dot result values feeding a high-bit mask by
default. Set `FUZZX_ALLOW_M053_BYTEDOT_HIGHBIT=1` to re-enable this bug class.
