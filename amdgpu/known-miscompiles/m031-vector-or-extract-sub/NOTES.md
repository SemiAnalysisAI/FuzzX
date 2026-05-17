# m031: Optimized vector `or` extract/sub scalarizes with the wrong lane value

Found while fuzzing the ROCm 7.2.3 source build after enabling vector
subexpressions in the LLVM-bitcode C++ fuzzer. The original fuzzer input
reduced to a two-lane vector `or` followed by two extracts and a subtract.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m031-vector-or-extract-sub/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
[0] input=0xc0d873b1 O0=0xc0d87400 O2=0x0000004e mismatch=true
any_mismatch=true
```

## Reduction

For `%v == 0xc0d873b1`, the reduced IR computes:

```llvm
%va1 = insertelement <2 x i32> <i32 0, i32 -1>, i32 %v, i32 0
%vb1 = insertelement <2 x i32> <i32 255, i32 0>, i32 %v, i32 1
%or = or <2 x i32> %va1, %vb1
%e0 = extractelement <2 x i32> %or, i32 0
%e1 = extractelement <2 x i32> %or, i32 1
%sub = sub i32 %e0, %e1
```

The vector values are `<%v, -1>` and `<255, %v>`, so `%or` must be
`<0xc0d873ff, 0xffffffff>`. The final subtraction is therefore
`0xc0d873ff - 0xffffffff == 0xc0d87400`.

## Root Cause Notes

The ROCm 7.2.3 `-O2` pipeline scalarizes the vector expression into:

```asm
v_or_b32_e32 v2, 0xff, v1
v_sub_u32_e32 v1, v2, v1
global_store_dword v0, v1, s[2:3]
```

This computes `(v | 255) - v`. The second extracted lane should be the lane-1
result of the vector `or`, namely `-1 | v == -1`, not the original `%v`.

The `-O0` path materializes lane 1 as `-1`, ORs it with `%v`, and subtracts
that lane value, producing the expected `0xc0d87400`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0xc0d87400`, `O2=0x0000004e`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Passes: `O0=0xc0d87400`, `O2=0xc0d87400`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Passes: `O0=0xc0d87400`, `O2=0xc0d87400`. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses subtracting two different scalar extracts
from the same vector `or`. Set `FUZZX_ALLOW_M031_VECTOR_OR_EXTRACT_SUB=1` to
re-enable this shape when replaying the original fuzzer input.
