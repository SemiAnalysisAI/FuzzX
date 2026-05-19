# m065-red-global-min-loop-fold

Found while replaying the post-global-atomics/generic-memory sweep after
suppressing the known m002-style `lop3`/`xor` fold family:

```text
divergences/active-20260519-060903-ptxas-13.2.78-post-lop3-xor-suppress-replay/div-1779170957-18b0e21c3783e094
```

The reduced program keeps a loop-carried value that is stored to this thread's
global output slice, followed by:

```ptx
st.global.u32      [%rd8 + 8], %r6;
@%p21 red.global.min.u32 [%rd8 + 8], %r0;
```

At `-O0`, the final loop iteration leaves output slot 2 as `0x1d`. At `-O3`,
threads 0 through 8 store `0x04` in that slot instead, as if optimized ptxas
used an earlier loop value for the store/reduction pair.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m065-red-global-min-loop-fold/reduced.ptx \
  known-miscompiles/m065-red-global-min-loop-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 9/32 tids differ, 9/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use
`DIV_DISABLE_GLOBAL_REDUCTIONS=1`.
