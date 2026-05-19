# m064-scalar16-max-cvt-predicate-fold

Found while continuing the post-`m063` sweep with scalar 16-bit `min` already
disabled:

```text
divergences/active-20260519-021511-post-m063/div-1779157189-000000006a0e4dd1
```

The original fuzzer program diverged in several live output slots. A focused
reduction observes the predicate directly:

```text
cvt.u16.u32   %h0, %r14
cvt.u16.u32   %h1, 33554432
max.u16       %h2, %h0, %h1
cvt.u32.u16   %r6, %h2
setp.eq.u32   %p89, 0, %r6
selp.u32      %r0, 1, 0, %p89
```

Both `-O0` and `-O3` materialize `%r6 = 0x20` in the manual instrumented build,
but affected optimized ptxas treats the following zero predicate as true. The
checked-in reducer stores `0` at `-O0` and `1` at `-O3` for every thread.

```bash
target/release/fuzzx-diff-test \
  known-miscompiles/m064-scalar16-max-cvt-predicate-fold/reduced.ptx \
  known-miscompiles/m064-scalar16-max-cvt-predicate-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 32/32 tids differ, 32/128 u32 slots differ
```

This appears to be the scalar `max.u16` side of the same family as
`m058-scalar16-min-cvt-fold`, where scalar 16-bit min/max through `.b16` scratch
registers corrupts a later predicate fold.

This reproduced on 2026-05-19 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued fuzzing past this family, use `DIV_DISABLE_SCALAR_16BIT_MIN=1`;
the post-known profile now treats that knob as scalar 16-bit min/max
suppression.
