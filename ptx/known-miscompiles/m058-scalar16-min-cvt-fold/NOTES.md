# m058-scalar16-min-cvt-fold

Found while continuing the scalar 16-bit sweep after suppressing
`m057-s16-unary-intmin-fold`:

```text
divergences/focused-packed-scalar16-no-signed-unary/div-1779097635-18b09fd79c2c50e8
```

The original program also contained packed halfword min/max, but reduction
showed the packed operation was incidental. The reduced trigger is scalar
`min.s16` or `min.u16` through `.b16` scratch registers, converted back to
`u32`, and used to fold a predicate.

## Scalar Trace

For the signed checked-in reproducer, `reduced.ptx`:

```text
n = 32
a = cvt.s16.s32 4
b = cvt.s16.s32 n
m = min.s16 a, b
out = cvt.s32.s16 m
p = (out == 4)
```

The correct result is `out == 4`, so the guarded `mad.lo` is not executed and
the kernel stores the input word. Affected optimized ptxas behaves as if the
predicate were false and stores `26 * tid` instead. `reduced_u16.ptx` uses the
same shape with `min.u16` and `cvt.u32.u16`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-18 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued fuzzing past this family, use `DIV_DISABLE_SCALAR_16BIT_MIN=1`.
