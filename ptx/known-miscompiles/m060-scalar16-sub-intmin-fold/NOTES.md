# m060-scalar16-sub-intmin-fold

Found while fuzzing the `ldu.global` and typed `selp` generator paths after
suppressing `m059-scalar16-pred-mulwide-fold`:

```text
divergences/active-20260518-230927-ldu-selp-rebased-m059-suppressed/div-1779145816-18b0cba618bfe37a
```

The original program did not depend on `ldu.global` or typed wide `selp`.
Reduction showed the trigger is a scalar signed 16-bit subtract through `.b16`
scratch registers, where the input is the signed 16-bit minimum value.

## Scalar Trace

For `reduced.ptx`, lane 14 has `%lanemask_gt == 0xffff8000`:

```text
h0 = cvt.s16.s32 0
h1 = cvt.s16.s32 lanemask_gt   # 0x8000, i.e. -32768 as s16
h2 = sub.s16 h0, h1            # 0x8000 after 16-bit wrap
r2 = cvt.s32.s16 h2            # 0xffff8000
r3 = r2 & 0x59d96384           # 0x59d90000
```

Affected optimized ptxas stores `0x00008000` for `r2` and zero for `r3` on
lane 14, as if the signed 16-bit subtract result were zero-extended before the
following signed conversion.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-18 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued fuzzing past this family, use `DIV_DISABLE_SIGNED_SCALAR_16BIT=1`.
