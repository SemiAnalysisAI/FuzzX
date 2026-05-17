# m028: `umax` masked by a value and its complement keeps a bit at `-O0`

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m028-umax-and-not/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
[2] input=0x7fffffff O0=0x00000002 O2=0x00000000 mismatch=true
```

## Reduction

For work-item 2 and input `0x7fffffff`, the reduced IR computes:

```llvm
%mix = xor i32 %v, %salt
%not = xor i32 %mix, -1
%shl = shl i32 %wi, 15
%masked = and i32 %shl, %not
%umax = call i32 @llvm.umax.i32(i32 %masked, i32 2)
%and0 = and i32 %umax, %mix
%result = and i32 %and0, %not
```

At `%wi == 2`, `%mix` is `0x43910c8d`, `%not` is `0xbc6ef372`, and
`%masked` is zero. `%umax` is therefore two. Since bit 1 is clear in `%mix`
and set in `%not`, `(2 & %mix) & %not` must be zero.

## Root Cause Notes

ROCm 7.2.3 `-O0` lowers the tail through two `v_bitop3_b32` instructions:

```asm
v_lshlrev_b32_e64 v2, 15, v2
v_bitop3_b32 v2, v2, v3, v4 bitop3:0x90
v_max_u32_e64 v2, v2, 2
v_bitop3_b32 v2, v2, v3, v4 bitop3:0x80
```

Here `v3` is the loaded input and `v4` is the salt, so the final bit operation
uses those operands separately instead of the derived `%mix` and `%not` values.
It keeps bit 1 and stores `0x00000002`. The `-O2` pipeline simplifies the
expression to zero and stores `0x00000000`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000002`, `O2=0x00000000`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: `O0=0x00000002`, `O2=0x00000000`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0x00000002`, `O2=0x00000000`. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses
`(umax((y & ~x), C) & x) & ~x` shapes by default. Set
`FUZZX_ALLOW_M028_UMAX_AND_NOT=1` to re-enable this shape.
