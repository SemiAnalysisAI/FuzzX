# m029: `fshl`-masked complement feeding a signed select picks the wrong arm at `-O2`

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer after enabling nested CFG generation.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m029-fshl-select-phi/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000000
O0=0xfffffffe
O2=0x00000000
mismatch=true
```

## Reduction

For work-item 0 with one input element, `%n` is one and `%wi` is zero. The
reduced IR computes:

```llvm
%fshl = call i32 @llvm.fshl.i32(i32 %n, i32 %n, i32 14)
%mask = add i32 %wi, 32
%masked = and i32 %fshl, %mask
%x = xor i32 %masked, -1
...
%p = phi i32 [ %one, %then ], [ 0, %else ]
%sum = add i32 %p, %wi
%y = xor i32 %sum, %x
%cmp = icmp sgt i32 %y, %x
%and = and i32 %y, %x
%result = select i1 %cmp, i32 0, i32 %and
```

At lane 0, `%fshl` is `0x00004000`, `%mask` is `32`, `%masked` is zero, and
`%x` is `0xffffffff`. The branch produces `%p == 1`, so `%y` is
`0xfffffffe`. The signed compare `-2 > -1` is false, so the select must choose
`%and == 0xfffffffe`.

## Root Cause Notes

The ROCm 7.2.3 `-O2` pipeline lowers the tail through a bit-operation compare
and conditional mask:

```asm
s_lshr_b32 s2, s3, 18
v_add_u32_e32 v1, 32, v0
v_max_u32_e32 v0, 1, v0
v_bitop3_b32 v4, s2, v1, s2 bitop3:0x3f
v_bitop3_b32 v5, v0, s2, v1 bitop3:0x87
v_bitop3_b32 v0, v0, s2, v1 bitop3:0x84
v_cmp_le_u32_e32 vcc, v5, v4
v_cndmask_b32_e32 v0, 0, v0, vcc
```

For lane 0 this sequence stores zero, which corresponds to the true arm of the
select. The IR signed comparison is false, and `-O0` stores the expected
`0xfffffffe`. Storing the compare result or the select's false operand directly
does not reproduce, which points at the optimized compare/select lowering rather
than the arithmetic feeding it.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0xfffffffe`, `O2=0x00000000`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces: `O0=0xfffffffe`, `O2=0x00000000`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0xfffffffe`, `O2=0x00000000`. |

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses signed compare/select or compare/PHI
shapes where `y & x` is selected based on a signed comparison between `y` and
`x`, with `x` being the complement of a masked `llvm.fshl.i32` result. Set
`FUZZX_ALLOW_M029_FSHL_SELECT_PHI=1` to re-enable this shape.
