# m021: dynamic `(a | b) ^ a` after `fshl` is miscompiled

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m021-fshl-or-xor/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000001
O0=0x00000001
O2=0x00000000
mismatch=true
```

## Reduction

For the reproducer input, `%mix` is `0x9e3779b8`, so `%ashr` is
`0xffffff9e`. The low bit of `%ashr` is zero, therefore `%and` is zero:

```llvm
%mix = xor i32 %v, -1640531527
%ashr = ashr i32 %mix, 24
%and = and i32 %ashr, %v
```

The remaining expression shifts that zero through `fshl` and then computes:

```llvm
%xor = xor i32 %fshl, %and
%or = or i32 %xor, %and
%xor1 = xor i32 %or, %xor
```

Since both `%fshl` and `%and` are zero, the result is defined and should be
zero.

## Root Cause Notes

At `-O0`, ROCm 7.2.3 lowers the tail through:

```asm
v_ashrrev_i32_e64 v3, 24, v2
v_and_b32_e64 v2, v3, v4
v_lshlrev_b32_e64 v2, 5, v2
v_bitop3_b32 v2, v2, v3, v4 bitop3:0x82
```

For input `1`, `v3` is `0xffffff9e` and `v4` is `1`; the defined result is
zero, but this sequence stores one. The `-O2` pipeline rewrites the expression
before lowering and stores zero.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000001`, `O2=0x00000000`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |

## Fuzzer Follow-Up

The old fuzzer suppression for the generalized dynamic `(a | b) ^ a` idiom was
removed after llvm/llvm-project#198419 fixed this case for the current HEAD
campaigns.
