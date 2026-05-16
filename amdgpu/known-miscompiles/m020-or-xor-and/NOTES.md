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
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses the `((a | b) ^ b) & (a | b)` idiom by
default. Set `FUZZX_ALLOW_M020_OR_XOR_AND=1` to re-enable this shape.
