# m048: `<8 x i8>` saturating add loop shifts vector-reduce result by two

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
index=0
input=0x00000000
O0=0x363f2ff8
O2=0x363f2ffa
expected=0x363f2ff8
```

The reduced reproducer uses one full 256-lane workgroup:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m048-v8i8-uadd-sat-vecreduce-loop/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[0] input=0x00000000 O0=0x363f2ff8 O2=0x363f2ffa mismatch=true
any_mismatch=true
```

## Reduction

The reduced program builds a packed byte value, inserts bytes derived from the
loop-carried accumulator into two `<8 x i8>` vectors, applies
`llvm.uadd.sat.v8i8`, extracts two lanes, and feeds the result through a bit
smear plus a two-lane vector-reduce xor/and idiom. The final value is stored
after two outer loop iterations.

The reproducer's `RUN-INPUTS` line supplies 256 zero inputs so every launched
workitem has valid input and output storage.

## Root Cause Notes

For lane 0, O0 and the interpreter oracle produce `0x363f2ff8`; O2 produces
`0x363f2ffa`, differing by two in the low bits. ROCm 7.2.3 passes the reduced
reproducer, while LLVM HEAD and ROCm HEAD both fail with the local PR patches
applied, so this appears to be another HEAD-side optimization/lowering
regression rather than a release-branch bug.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: no `-O0` / `-O2` mismatch across 256 zero inputs. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: first mismatch is `[0] O0=0x363f2ff8`, `O2=0x363f2ffa`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: first mismatch is `[0] O0=0x363f2ff8`, `O2=0x363f2ffa`. |

## Fuzzer Follow-Up

The fuzzer now rejects `llvm.uadd.sat.v8i8` shapes by default. Set
`FUZZX_ALLOW_M048_V8I8_UADD_SAT=1` to re-enable this bug class.
