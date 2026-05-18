# m038: nested xor loop plus masked FP round-trip returns 1023 at O2

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer in oracle-required mode after enabling LLVM integer bit intrinsics. The
original candidate was an oracle finding: `-O0` matched the interpreter, while
`-O2` returned `1023`.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m038-loop-fp-mask-xor/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000000
O0=0x00000000
O2=0x000003ff
mismatch=true
```

## Reduction

The reduced kernel runs one work-item for input `0`. It first computes a
defined zero through a masked integer-to-float multiply:

```llvm
%mask.input = and i32 %v, 1023
%mask.block = and i32 %wg, 1023
%fp.input = uitofp i32 %mask.input to float
%fp.block = uitofp i32 %mask.block to float
%fp.mul = fmul float %fp.input, %fp.block
%fp.zero = fptoui float %fp.mul to i32
```

For workgroup zero and `%v == 0`, `%fp.zero` is exactly zero. The nested loop
starts from `%v | -1`, repeatedly computes `(acc & 1023) + %mask.input` through
an exact FP round-trip, xors it with the accumulator, and exits with
`%outer.acc == 0xfffffc00`. The final masked result is therefore zero.

All integer and FP operations are defined for every input used by the
reproducer: masks keep FP conversions small and exact, loop trip counts are
bounded, and the final integer add cannot overflow.

## Root Cause Notes

The ROCm 7.2.3 `-O0` assembly keeps the loop structure and stores the value
after explicit conversions:

```asm
v_and_b32_e64 v4, v2, s0
v_and_b32_e64 v2, v3, s0
v_cvt_f32_u32_e64 v4, v4
v_cvt_f32_u32_e64 v2, v2
v_cvt_f64_f32_e64 v[4:5], v4
v_cvt_f64_f32_e64 v[6:7], v2
v_add_f64 v[4:5], v[4:5], v[6:7]
v_cvt_f32_f64_e64 v2, v[4:5]
v_cvt_u32_f32_e64 v2, v2
v_add_u32_e64 v2, v2, v3
global_store_dword v[0:1], v2, off
```

The `-O2` pipeline unrolls the loops and folds the final masked FP round-trip
into integer code. In the reduced case, the generated code includes a byte-dot
sequence with a constant `0x3ff` accumulator:

```asm
s_movk_i32 s0, 0x3ff
v_perm_b32 v4, v1, v1, s1
v_bitop3_b32 v1, v1, s0, v3 bitop3:0x48
v_dot4_u32_u8 v2, v4, v2, s0
v_add_u32_e32 v1, v2, v1
global_store_dword v0, v1, s[6:7]
```

For input zero, the dot-product operands are zero, but the accumulator is
`1023`, so `-O2` stores `0x000003ff`. This points at an AMDGPU `-O2` combine or
lowering bug around the optimized masked integer/FP round-trip after loop
unrolling.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000000`, `O2=0x000003ff`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: `O0=0x00000000`, `O2=0x000003ff`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0x00000000`, `O2=0x000003ff`. |

Original fuzzer input SHA-1:

```text
1507fc8a9258b5d662764add8198c9130888ad0c
```

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses the final masked FP round-trip shape by
default when the rounded value is added back to one of the masked operands. Set
`FUZZX_ALLOW_M038_LOOP_FP_MASK_XOR=1` to re-enable this pattern when replaying
or intentionally fuzzing this combine family.
