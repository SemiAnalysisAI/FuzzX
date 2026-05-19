# m045: known-24-bit `urem x, (x | 1)` returns all low bits set

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
input=0x8891aa4d
O0=0x00002bb6
O2=0xffffccc3
expected=0x00002bb6
```

The first divergent value in the reduced fuzzer program was a remainder:

```text
%fuzz.loop.nest.acc = 0x00bf2758
%fuzz.urem          = 0x00bf2758 at -O0
%fuzz.urem          = 0x00ffffff at -O2
```

The standalone reproducer makes the required range information explicit:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m045-urem-or-one-known24/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
input=0x00bf2758
O0=0x00bf2758
O2=0x00ffffff
mismatch=true
```

## Reduction

For the recorded input, `%x < 2^24` is true, so the `llvm.assume` is satisfied.
The denominator is `%x | 1`, which is `0x00bf2759`. Since
`0x00bf2758 < 0x00bf2759`, the unsigned remainder must be `%x`.

The same direct `urem x, (x | 1)` expression passes when the compiler has no
range information for `%x`. If `%x` is produced by `and raw, 0x00ffffff`, both
`-O0` and `-O2` can return `0x00ffffff`; the original fuzzer case only showed an
`-O2` mismatch because `-O2` inferred the 24-bit range and `-O0` did not.

## Root Cause Notes

This appears to be in the known-24-bit unsigned remainder lowering for
AMDGPU. Once the numerator is known to fit in 24 bits, LLVM lowers
`urem x, (x | 1)` through a path that returns the 24-bit all-ones value
`0x00ffffff` for an even numerator that is smaller than the odd denominator.

The byte-dot and average-difference operations in the original generated
program were downstream of the bad remainder and were not needed to reproduce
the first wrong value.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00bf2758`, `O2=0x00ffffff`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0x00bf2758`, `O2=0x00ffffff`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0x00bf2758`, `O2=0x00ffffff`. |

## Fuzzer Follow-Up

The fuzzer now rejects `urem x, (x | 1)` shapes by default. Set
`FUZZX_ALLOW_M045_UREM_OR_ONE=1` to re-enable this bug class.
