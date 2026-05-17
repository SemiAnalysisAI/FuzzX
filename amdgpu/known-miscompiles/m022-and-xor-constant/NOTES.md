# m022: `(x ^ C) & x` is miscompiled after a dynamic `and`

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m022-and-xor-constant/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
[0] input=0x00000000 O0=0x86122928 O2=0x86122928 mismatch=false
[1] input=0x00000000 O0=0x86122929 O2=0x86122928 mismatch=true
any_mismatch=true
```

## Reduction

The reproducer ignores the input values, but it needs two input words so the
runner launches work-item one. For `%wi == 1`, the reduced IR computes:

```llvm
%salt = add i32 %wi, -1640531528
%mix = xor i32 %wi, %salt
%x = and i32 %mix, %salt
%xor = xor i32 %x, 2041403025
%result = and i32 %xor, %x
```

For work-item one, `%salt` is `0x9e3779b9`, `%mix` and `%x` are
`0x9e3779b8`, and the defined result is:

```text
(0x9e3779b8 ^ 0x79ad5691) & 0x9e3779b8 == 0x86122928
```

## Root Cause Notes

At `-O0`, ROCm 7.2.3 lowers the tail to a single `v_bitop3_b32` over the
work-item id, `%salt`, and the xor constant:

```asm
v_add_u32_e64 v3, v2, 0x9e3779b8
v_bitop3_b32 v2, v2, v3, 0x79ad5691 bitop3:0x84
```

For bit zero of work-item one, all three bitop inputs are one, and
`bitop3:0x84` returns one. The original IR has `%x` bit zero clear, so
`(%x ^ C) & %x` must also have bit zero clear. This accounts for the observed
off-by-one result. The `-O2` pipeline rewrites the expression before lowering
and stores `0x86122928`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: lane 1 `O0=0x86122929`, `O2=0x86122928`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: lane 1 `O0=0x86122929`, `O2=0x86122928`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: lane 1 `O0=0x86122929`, `O2=0x86122928`. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses `((x ^ C) & x)` and its operand-swapped
form by default. Set `FUZZX_ALLOW_M022_AND_XOR_CONSTANT=1` to re-enable this
shape.
