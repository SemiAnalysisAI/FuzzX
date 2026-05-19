# m069-wide-subc-loop-borrow-fold

Found while replaying the post-m068 CUDA 13.2.78 sweep with global
reductions, global atomics, and register-operand bitfield forms disabled:

```text
divergences/active-20260519-072210-ptxas-13.2.78-post-m068-nored-noatom-noregbitfield/div-1779174481-18b0e49b5de13e9a
```

The line reducer preserved the divergence but removed the initializers for the
first 64-bit subtract, so this repro keeps the original valid generator program.
The interesting block computes a 64-bit subtract and consumes its borrow in a
second 64-bit subtract:

```ptx
cvt.u64.u32  %rd4, %r5;
cvt.u64.u32  %rd5, 26;
cvt.u64.u32  %rd6, %r1;
cvt.u64.u32  %rd7, 15;
@%p22 sub.cc.u64 %rd4, %rd4, %rd5;
@%p22 subc.u64   %rd6, %rd6, %rd7;
@%p22 mov.b64    {%r0, %r13}, %rd6;
```

The final output stores `%r0`. At `-O0`, lanes 8 through 16 store `0xfffffff2`
and lanes 17 through 31 store `0xfffffff1`. At `-O3`, those lanes store the
lane-dependent value from `%r1 - 15`, matching a path where the optimized code
does not preserve the loop-carried borrow/value state feeding the `subc.u64`
result.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m069-wide-subc-loop-borrow-fold/reduced.ptx \
  known-miscompiles/m069-wide-subc-loop-borrow-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 24/32 tids differ, 24/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_WIDE_SUBC=1`.
