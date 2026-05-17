# m026: `umax` high-bit extraction after `shl` returns all ones at `-O2`

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m026-shl-umax-xor-and/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x83cdfead
O0=0x00000000
O2=0xffffffff
mismatch=true
```

## Reduction

For work-item zero, the reduced IR computes:

```llvm
%salt = add i32 %wi, -239350328
%mix = xor i32 %v, %salt
%shl = shl i32 %mix, 8
%umax = call i32 @llvm.umax.i32(i32 %shl, i32 %mix)
%xor = xor i32 %umax, %mix
%and = and i32 %xor, %umax
%result = ashr i32 %and, 31
```

At `%wi == 0` and input `0x83cdfead`, `%salt` is `0xf1bbcdc8`, `%mix` is
`0x72763365`, `%shl` is `0x76336500`, and `%umax` is `0x76336500`. Therefore
`%and` is `0x04014400`, whose sign bit is clear, so the arithmetic shift by
31 must return zero.

## Root Cause Notes

The `-O2` pipeline keeps the expression as a `llvm.umax.i32` high-bit
extraction:

```llvm
%select = tail call i32 @llvm.umax.i32(i32 %shl, i32 %mix)
%not.mix = xor i32 %mix, -1
%and = and i32 %select, %not.mix
%result = ashr i32 %and, 31
```

ROCm 7.2.3 `-O2` lowers the tail through:

```asm
v_lshlrev_b32_e32 v3, 8, v3
v_bitop3_b32 v0, v3, v2, v0 bitop3:0x98
v_ashrrev_i32_e32 v0, 31, v0
```

Here `v3` has the shifted value, while `v2` and `v0` still hold the loaded input
and salt. The bitop therefore observes their high bits separately instead of the
high bit of `%mix = %v ^ %salt`. For this input, the input and salt both have
the high bit set, but `%mix` does not. The `-O0` pipeline keeps the `%mix`
value through a `v_max_u32` plus `v_bitop3_b32 ... bitop3:0x48` sequence and
stores zero. The optimized sequence stores `0xffffffff`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000000`, `O2=0xffffffff`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: `O0=0x00000000`, `O2=0xffffffff`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0x00000000`, `O2=0xffffffff`. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses `ashr` high-bit extraction from
`(umax(a, b) ^ b) & umax(a, b)` shapes by default. This catches both explicit
`llvm.umax.i32` and the equivalent `select` over an unsigned compare. Set
`FUZZX_ALLOW_M026_UMAX_XOR_AND_HIGHBIT=1` to re-enable this shape.
