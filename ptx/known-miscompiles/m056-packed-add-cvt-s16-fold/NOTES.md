# m056-packed-add-cvt-s16-fold

Found during an unsigned packed-add follow-up sweep after disabling signed
packed add for `m054-packed-add-cvt-fold`:

```text
/tmp/fuzzx-packed-add-unsigned-only-1779082682/div-1779084897-18b0944602de5472
```

This is high confidence the same root-cause family as
`m054-packed-add-cvt-fold`: a packed add feeding a subword conversion loses the
packed-add contribution under optimization. Here the trigger is unsigned
packed add plus signed 16-bit conversion.

## Scalar Trace

For the checked-in reproducer:

```text
n = 32
t0 = add.u16x2 28, n
t1 = cvt.s32.s16 t0
out = n + t1
```

The low halfword of `t0` is `60`, so the correct `-O0` output is `92`
(`0x0000005c`). Affected optimized ptxas stores `64` (`0x00000040`), as if
the `+28` from the packed add was dropped before the conversion.

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
