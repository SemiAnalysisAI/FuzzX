# m019: high-bit `(x | C) ^ x` is miscompiled through `v_bitop3_b32`

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m019-highbit-or-xor/reduced.ll
```

Observed result:

```text
input=0x00000000
O0=0x80000000
O2=0x00000000
mismatch=true
```

## Reduction

The reduced IR computes:

```llvm
%salt = xor i32 %workitem.id.x, 0x80000000
%mix = xor i32 %load, %salt
%and = and i32 %mix, %salt
%or = or i32 %and, 0x80000000
%xor = xor i32 %or, %and
```

For the single executed work-item and input `0`, `%salt` and `%and` are both
`0x80000000`, so `%xor` should be zero.

## Root Cause Notes

At `-O2`, the expression is simplified to `load & 0x80000000`, which returns
zero for the reproducer input.

At `-O0`, instruction selection combines the bit expression into:

```asm
v_bitop3_b32 v2, v2, v3, s2 bitop3:0x62
```

where `v2` is the loaded input, `v3` is the work-item id, and `s2` is
`0x80000000`. For work-item zero and input zero this instruction produces
`0x80000000`, but the LLVM IR expression is defined and evaluates to zero.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |

## Fuzzer Follow-Up

The old fuzzer suppression for the outer high-bit `(x | C) ^ x` idiom was
removed after llvm/llvm-project#198419 fixed this case for the current HEAD
campaigns.
