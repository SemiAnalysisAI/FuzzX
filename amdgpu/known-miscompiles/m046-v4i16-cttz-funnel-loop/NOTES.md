# m046: vector `cttz.v4i16` loop returns the wrong O2 accumulator

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
index=0
input=0x00000000
O0=0xfffffffc
O2=0xfffffffb
expected=0xfffffffc
```

The reduced reproducer uses one full 256-lane workgroup:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m046-v4i16-cttz-funnel-loop/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[0] input=0x00000000 O0=0xfffffffc O2=0xfffffffe mismatch=true
any_mismatch=true
```

## Reduction

The live reduced program computes a nested loop with an inner trip count derived
from the loop-carried accumulator. Inside the inner loop, it builds a
`<4 x i16>` vector, applies `llvm.cttz.v4i16`, extracts a lane, uses the result
in a `<4 x i32>` vector add, then feeds a funnel-shift-shaped scalar expression.

The reproducer's `RUN-INPUTS` line supplies 256 zero inputs so every launched
workitem has valid input and output storage.

## Root Cause Notes

The O2 pipeline changes the final accumulator for lane 0 from `0xfffffffc` to
`0xfffffffe` in the reduced program. Replacing the vector `cttz`/funnel
expression with the scalar value it appears to compute for lane 0 makes the
reproducer pass, so the vector expression is part of the trigger rather than a
plain dynamic-trip-count loop being sufficient by itself.

This looks like a new HEAD-side optimization/lowering bug: ROCm 7.2.3 passes
the reduced reproducer, while LLVM HEAD and ROCm HEAD both fail with the local
PR patches applied.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: no `-O0` / `-O2` mismatch across 256 zero inputs. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: first mismatch is `[0] O0=0xfffffffc`, `O2=0xfffffffe`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: first mismatch is `[0] O0=0xfffffffc`, `O2=0xfffffffe`. |

## Fuzzer Follow-Up

The fuzzer now rejects `llvm.cttz.v4i16` shapes by default. Set
`FUZZX_ALLOW_M046_V4I16_CTTZ=1` to re-enable this bug class.
