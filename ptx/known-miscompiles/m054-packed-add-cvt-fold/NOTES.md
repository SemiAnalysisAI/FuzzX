# m054-packed-add-cvt-fold

Found during a packed-add sweep:

```text
/tmp/fuzzx-packed-add-1779082682/div-1779082760-18b0925452e73ff9
```

The automated reducer exposed the core value chain but also deleted a live
predicate definition. A manual live-value pass reduced the checked-in PTX to a
defined three-instruction sequence.

`m056-packed-add-cvt-s16-fold` is high confidence the same root-cause family:
it uses unsigned packed add and signed 16-bit conversion, but the optimized
code drops the packed-add contribution in the same way.

## Scalar Trace

For the checked-in reproducer:

```text
input = 0x847d7c8d
t0 = add.s16x2 4, input
t1 = cvt.u32.u16 t0
out = add.s16x2 t1, input
```

The low halfword of `t0` is `0x7c91`, so the correct `-O0` output is
`0x847df91e`. Affected optimized ptxas stores `0x847df91a`, as if the `+4`
inside the packed halfword add was dropped before the final packed add.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-18 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

Retested on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas
(`release 13.2, V13.2.78`): the checked-in reducer's `-O0` and `-O3`
outputs match, so this is listed as fixed in 13.2.78.

For continued fuzzing past this family, use `DIV_DISABLE_PACKED_ADD=1`.
