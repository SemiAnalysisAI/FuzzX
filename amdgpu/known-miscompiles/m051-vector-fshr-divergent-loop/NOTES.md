# m051: vector `fshr` loop scalarization reuses a scalar inner-loop value

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=1
input=0x1
o0=0x871C1966
o2=0x871C1967
expected=0x871C1966
```

The reduced reproducer uses two inputs so workitem 1 reaches the divergent
nested-loop path:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m051-vector-fshr-divergent-loop/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[0] input=0x00000000 O0=0x871c1a66 O2=0x871c1a66 mismatch=false
[1] input=0x00000001 O0=0x871c1a66 O2=0x871c1a67 mismatch=true
any_mismatch=true
```

The fuzzer's LLVM-interpreter oracle also reports that `0x871c1a66` is the
expected value for input 1.

## Reduction

The reduced program computes a divergent outer trip count and starting value
from `input ^ (workitem.id.x * 0x9e3779b9)`. Inside the nested loop, the loop
body creates a `<2 x i32>` vector, calls `llvm.fshr.v2i32`, extracts lane 0,
truncates to `i8`, multiplies by 85, sign-extends, and ORs a constant. The
result is XORed with the inner-loop induction variable before feeding the next
loop iteration and the next outer iteration.

## Root Cause Notes

At `-O2`, LLVM simplifies the vector-funnel and narrow multiply tail to a
scalar constant expression. In the generated assembly, the inner loop computes
`s7 = s8 ^ 0x871c1a66`, where `s8` is a scalar inner-loop IV. The loop uses EXEC
masking for lanes with different inner trip counts, and `v3` keeps the correct
per-lane final value for the current inner loop.

However, the value carried into the next outer iteration is updated from scalar
`s7` for all lanes after EXEC is restored, instead of from the per-lane final
value. Lanes whose inner loops exited before the maximum active trip count can
therefore carry the wrong value into the next outer iteration. In this
reproducer, workitem 1 eventually stores `0x871c1a67` at `-O2`; the interpreter
and `-O0` both produce `0x871c1a66`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: no `-O0` / `-O2` mismatch across inputs `0x0 0x1`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: input 1 gives `O0=0x871c1a66`, `O2=0x871c1a67`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: input 1 gives `O0=0x871c1a66`, `O2=0x871c1a67`. |

## Fuzzer Follow-Up

The fuzzer now rejects vector `llvm.fshr` calls by default. Set
`FUZZX_ALLOW_M051_VECTOR_FSHR_LOOP=1` to re-enable this bug class.
