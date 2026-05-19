# m020: `((a | b) ^ b) & (a | b)` is miscompiled through `v_bitop3_b32`

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m020-or-xor-and/reduced.ll
```

Observed result:

```text
input=0x00000000
O0=0x00000000
O2=0x00000018
mismatch=true
```

## Reduction

The reduced IR computes this expression for work-item zero:

```llvm
%and = or i32 %wi, 3
%and1 = add i32 %wi, 25
%or3 = or i32 %and1, %and
%xor1 = xor i32 %or3, %and
%and3 = and i32 %xor1, %or3
```

For `%wi == 0`, `%and` is `3`, `%and1` is `25`, and `%or3` is `27`.
Therefore `%and3` is `((27 ^ 3) & 27) == 24`.

## Root Cause Notes

At `-O0`, instruction selection combines the bit expression into:

```asm
v_add_u32_e64 v2, v3, s0
v_bitop3_b32 v2, v2, v3, s0
```

where `v3` is the work-item id and `s0` is `3`. For work-item zero, this
stores zero. The IR expression is defined and should store `0x18`.

At `-O2`, the expression is rewritten before lowering and the generated code
stores `0x18` for the reproducer.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000018`, `O2=0x00000018`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000018`, `O2=0x00000018`. |

## Fuzzer Follow-Up

The old fuzzer suppression for the `((a | b) ^ b) & (a | b)` idiom was
removed after llvm/llvm-project#198419 fixed this case for the current HEAD
campaigns.
