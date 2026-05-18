# m057-s16-unary-intmin-fold

Found after adding scalar 16-bit ALU generation through `.b16` scratch
registers:

```text
divergences/focused-packed-scalar16-combined/div-1779095094-18b09d8e5e7943aa
```

The original hit also contained packed halfword min/max instructions, but
manual reduction showed those were incidental. The reduced trigger is a signed
16-bit unary op (`abs.s16` or `neg.s16`) on the signed 16-bit minimum value
followed by `cvt.s32.s16`.

## Scalar Trace

For the primary checked-in reproducer, `reduced.ptx`:

```text
tid = 0
x = tid + 32768
h0 = cvt.s16.s32 x     # 0x8000, i.e. -32768 as s16
h2 = abs.s16 h0        # same bit pattern for neg.s16
y = cvt.s32.s16 h2     # 0xffff8000
out = x + y
```

`reduced_neg.ptx` uses the same sequence with `neg.s16` in place of
`abs.s16` and reproduces the same output difference.

The correct `-O0` output for lane 0 is `0x00000000`. Affected optimized ptxas
stores `0x00010000`, as if the unary result were known to be non-negative and
could be zero-extended through the following signed conversion.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-18 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued fuzzing past this family, use `DIV_DISABLE_SCALAR_16BIT_SIGNED_UNARY=1`.
