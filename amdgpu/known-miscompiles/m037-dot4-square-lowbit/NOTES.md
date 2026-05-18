# m037: byte-masked square plus low bit lowered to wrong dot product

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer in oracle-required mode. The original generated program included scalar
FP conversions, a multi-exit loop, and vector extracts, but the active
miscompile reduces to a byte-masked integer square.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m037-dot4-square-lowbit/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000000
O0=0x00000000
O2=0x00000001
mismatch=true
```

## Reduction

Only work-item zero stores, so the testcase avoids a race on the single output
word. For the recorded input `0`, the reduced IR computes:

```llvm
%masked = and i32 %v, 255
%square = mul i32 %masked, %masked
%lowbit = and i32 %square, 1
%result = add i32 %lowbit, %square
```

All operations are defined for every `i32` input: `%masked` is at most `255`,
so `%square` is at most `65025`, and the final add cannot overflow.

## Root Cause Notes

The ROCm 7.2.3 `-O0` assembly implements the expression directly:

```asm
s_and_b32 s2, s2, 0xff
s_mul_i32 s3, s2, s2
s_and_b32 s2, s3, 1
s_add_i32 s2, s2, s3
```

The ROCm 7.2.3 `-O2` assembly rewrites the expression into a byte dot product:

```asm
v_mov_b32_e32 v1, 0xc0c0000
v_perm_b32 v1, s0, s0, v1
v_dot4_u32_u8 v1, v1, v1, 1
```

For `%v == 0`, this sequence still returns `1` because the dot-product
accumulator is the constant `1`. For `%v == 1`, the same sequence returns `3`
instead of `2`, because the `v_perm_b32` operand duplicates the low byte and the
dot product adds both byte squares plus the constant accumulator. This points at
an AMDGPU `-O2` combine/lowering bug for the `x*x + (x*x & C)` shape after byte
masking.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000000`, `O2=0x00000001`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: `O0=0x00000000`, `O2=0x00000001`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0x00000000`, `O2=0x00000001`. |

Original fuzzer input SHA-1:

```text
518e83aa0872a55f8253bb40822134bc98c6c313
```

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses byte-masked square expressions of the form
`(x * x) + ((x * x) & C)` by default. Set
`FUZZX_ALLOW_M037_DOT4_SQUARE_LOWBIT=1` to re-enable this pattern when replaying
or intentionally fuzzing the dot-product combine family.
