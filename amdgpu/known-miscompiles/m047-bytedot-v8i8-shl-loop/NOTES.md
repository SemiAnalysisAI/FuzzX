# m047: byte-dot-style loop with `<8 x i8>` shift returns constant four

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
index=0
input=0x00000000
O0=0x00000002
O2=0x00000004
expected=0x00000002
```

The reduced reproducer uses one full 256-lane workgroup:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m047-bytedot-v8i8-shl-loop/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[0] input=0x00000000 O0=0x00000001 O2=0x00000004 mismatch=true
any_mismatch=true
```

## Reduction

The reduced program computes an average-difference-shaped value, then runs a
data-dependent inner loop. The loop body contains byte-dot-style arithmetic,
packs alternating dynamic and workgroup-id bytes into a `<8 x i8>` vector,
shifts that vector by a constant vector, and uses two extracted lanes to produce
the next accumulator. A final signed-overflow-select idiom stores the result.

The reproducer's `RUN-INPUTS` line supplies 256 zero inputs so every launched
workitem has valid input and output storage.

## Root Cause Notes

On the reduced program, O2 returns `0x00000004` for lane 0 where O0 returns
`0x00000001`. Several lanes that O0 computes as `1`, `2`, or `3` are also folded
to `4` by O2. This points at the optimized loop/body expression rather than a
release-branch lowering bug: ROCm 7.2.3 passes the reduced reproducer, while
LLVM HEAD and ROCm HEAD both fail with the local PR patches applied.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: no `-O0` / `-O2` mismatch across 256 zero inputs. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: first mismatch is `[0] O0=0x00000001`, `O2=0x00000004`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: first mismatch is `[0] O0=0x00000001`, `O2=0x00000004`. |

## Fuzzer Follow-Up

The fuzzer now rejects `<8 x i8>` vector `shl` shapes by default. Set
`FUZZX_ALLOW_M047_V8I8_SHL=1` to re-enable this bug class.
