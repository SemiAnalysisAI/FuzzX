# m023: `(x & y) ^ x` identity is miscompiled through `v_bitop3_b32`

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m023-and-xor-identity/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000000
O0=0x00000020
O2=0x00000029
mismatch=true
```

## Reduction

For work-item zero, the reduced IR computes:

```llvm
%same0 = add i32 %wi, 32
%a = or i32 %same0, 9
%same1 = sub i32 32, %wi
%b = xor i32 %same0, %same1
%x = xor i32 %a, %b
%and = and i32 %x, %b
%result = xor i32 %and, %x
```

At `%wi == 0`, `%same0` and `%same1` are both `32`, so `%b` is zero. `%a`
and `%x` are both `0x29`, and `(x & 0) ^ x` must therefore be `0x29`.

## Root Cause Notes

At `-O0`, ROCm 7.2.3 combines the tail into:

```asm
v_add_u32_e64 v3, v4, 32
v_or_b32_e64 v2, v3, 9
v_sub_u32_e64 v4, 32, v4
v_bitop3_b32 v2, v2, v3, v4 bitop3:0xca
```

For work-item zero, the bitop inputs are `%a == 0x29`, `%same0 == 0x20`,
and `%same1 == 0x20`. The original IR simplifies to
`%a & ~(%same0 ^ %same1)`, so the low bits from `%a` should survive. The
`bitop3:0xca` lowering clears those bits and stores only `0x20`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000020`, `O2=0x00000029`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000029`, `O2=0x00000029`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000029`, `O2=0x00000029`. |

## Fuzzer Follow-Up

The old fuzzer suppression for `(x & y) ^ x` and its operand-swapped form was
removed after llvm/llvm-project#198419 fixed this case for the current HEAD
campaigns.
