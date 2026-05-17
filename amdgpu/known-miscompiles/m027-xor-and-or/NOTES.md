# m027: nested xor/and feeding `or` is miscompiled through `v_bitop3_b32`

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer after enabling `i64` IR generation. The reduced reproducer itself is an
`i32` bitop pattern.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m027-xor-and-or/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
[2] input=0x00000000 O0=0xe0198e83 O2=0xe0198e81 mismatch=true
```

## Reduction

For work-item 2, the reduced IR computes:

```llvm
%mul = add i32 %wi, -535196033
%xor = xor i32 %mul, -1431655766
%x = and i32 %xor, %mul
%yxor = xor i32 %wi, %x
%and = and i32 %yxor, %x
%result = or i32 %and, %mul
```

At `%wi == 2`, `%mul` is `0xe0198e81`. Since
`%x = (%mul ^ 0xaaaaaaaa) & %mul`, every set bit in `%x` is already set in
`%mul`. Therefore `%and` is also a subset of `%mul`, and `%and | %mul` must be
`%mul`, namely `0xe0198e81`.

## Root Cause Notes

ROCm 7.2.3 `-O0` lowers the reduced tail to:

```asm
v_add_u32_e64 v3, v2, 0xe0198e7f
v_bitop3_b32 v2, v2, v3, 0xaaaaaaaa bitop3:0xec
global_store_dword ..., v2, ...
```

The combined `v_bitop3_b32` uses the work-item id, `%mul`, and the xor
constant directly. For work-item 2, it sets bit 1 and stores `0xe0198e83`.
The `-O2` pipeline simplifies the expression to `%mul` and stores
`0xe0198e81`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0xe0198e83`, `O2=0xe0198e81`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: `O0=0xe0198e83`, `O2=0xe0198e81`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0xe0198e83`, `O2=0xe0198e81`. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses the precise final shape
`(((y ^ x) & x) | base)` when `x` is itself `(base ^ z) & base`. Set
`FUZZX_ALLOW_M027_XOR_AND_OR=1` to re-enable this shape.
