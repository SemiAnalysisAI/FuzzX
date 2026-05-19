# m054: i64 pair add drops the high-half contribution

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=1
input=0x1
o0=0x1FF0003
o2=0xFF0002
expected=0x1FF0003
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m054-i64-pair-low-add/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[0] input=0x00000000 O0=0x0000ffff O2=0x0000ffff mismatch=false
[1] input=0x00000001 O0=0x01ff0003 O2=0x00ff0002 mismatch=true
any_mismatch=true
```

ROCm 7.2.3, LLVM HEAD, and ROCm HEAD all reproduce this reduced testcase.

## Reduction

The minimized kernel builds a 64-bit product from the workitem id and a value
derived from the launch count, copies that product into the high half of a
second 64-bit value, ORs in `0xffff`, adds the original product, and folds the
high and low halves:

```llvm
%prod = mul i64 %a64, %idx64
%pair.hi = shl i64 %prod, 32
%pair = or i64 %pair.hi, 65535
%sum = add i64 %pair, %prod
%hi = lshr i64 %sum, 32
%fold64 = xor i64 %hi, %sum
```

For lane 1 in the default two-element run, `%prod == 0x0000000000fffffe`.
Therefore `%sum == 0x00fffffe0100fffd`, and the folded low 32 bits are
`0x01ff0003`. LLVM HEAD `-O2` returns `0x00ff0002`.

## Root Cause Notes

At `-O0`, LLVM emits a full 64-bit construction/add/fold sequence and preserves
both the shifted high-half product and the low product.

At `-O2`, LLVM lowers the expression to a shorter u24 multiply-add shape:

```asm
s_and_b32 s0, s4, 2
s_sub_i32 s0, 0, s0
v_mul_u32_u24_e32 v1, s0, v0
v_mad_u64_u32 v[2:3], s[0:1], s0, v0, v[2:3]
v_bitop3_b32 v1, v3, v2, v1 bitop3:0x36
```

The folded result uses the low product but effectively drops the high-half copy
of `%prod` from `(%prod << 32) | 0xffff` when computing `%hi ^ %sum`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: lane 1 `O0=0x01ff0003`, `O2=0x00ff0002`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0x01ff0003`, `O2=0x00ff0002`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0x01ff0003`, `O2=0x00ff0002`. |

## Fuzzer Follow-Up

The fuzzer now rejects `((zext x << 32) | 0xffff) + zext x` pair-add shapes by
default. Set `FUZZX_ALLOW_M054_I64_PAIR_LOW_ADD=1` to re-enable this bug class.
