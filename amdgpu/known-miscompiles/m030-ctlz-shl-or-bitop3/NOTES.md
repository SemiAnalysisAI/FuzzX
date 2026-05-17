# m030: Optimized low-bit `or` lowers through `v_bitop3_b32` with unmasked input bits

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer after increasing CFG generation. The original fuzzer input reduced to a
small linear integer case.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m030-ctlz-shl-or-bitop3/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
[0] input=0x00000000 O0=0xefffc001 O2=0xefffc003 mismatch=true
[1] input=0x00000000 O0=0xefffc001 O2=0xefffc003 mismatch=true
any_mismatch=true
```

## Reduction

With two input elements, `%n` is two. The reduced IR computes:

```llvm
%shl.n = shl i32 %n, 31
%ctlz = call i32 @llvm.ctlz.i32(i32 %shl.n, i1 false)
%nz = icmp ne i32 %ctlz, 0
%z = zext i1 %nz to i32
%base = shl i32 2147467263, 14
%add = add i32 %base, %z
%result = or i32 %add, %z
```

For `%n == 2`, `%shl.n` is zero, `ctlz(0, false)` is 32, `%z` is one,
`%base` is `0xefffc000`, and both `%add` and `%result` must be
`0xefffc001`.

The original fuzzer input first diverged at an `or` whose operands were an
`llvm.smin.i32` result and the same small `%z` value. Storing either operand
alone did not reproduce. The minimized case shows that the `smin` is not
essential; the problematic tail is the redundant `or` of `z` after adding `z`
to a shifted value.

## Root Cause Notes

The ROCm 7.2.3 `-O2` pipeline simplifies the value feeding the `or` to:

```llvm
%fshl0.mask = and i32 %n, 1
%add = sub nuw nsw i32 -268451839, %fshl0.mask
%z = xor i32 %fshl0.mask, 1
%result = or i32 %add, %z
```

That IR is still semantically correct, but AMDGPU instruction selection lowers
the expression through:

```asm
v_mov_b32_e32 v1, 0xefffc001
v_bitop3_b32 v1, s2, 1, v1 bitop3:0x5e
```

Here `s2` is the full `%n` value. The optimized IR only depends on `%n & 1`,
but the selected `v_bitop3_b32` uses the unmasked `%n` bits. For `%n == 2`,
bit 1 leaks into the result, producing `0xefffc003` instead of
`0xefffc001`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0xefffc001`, `O2=0xefffc003`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: `O0=0xefffc001`, `O2=0xefffc003`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0xefffc001`, `O2=0xefffc003`. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses `or` tails where a shifted value plus a
small bit value is ORed with the same bit value, including the original
`smin(add(shl(...), z), z)` wrapper. Set
`FUZZX_ALLOW_M030_CTLZ_SHL_OR_BITOP3=1` to re-enable this shape.
