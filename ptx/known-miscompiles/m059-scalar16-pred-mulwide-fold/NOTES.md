# m059-scalar16-pred-mulwide-fold

Found while fuzzing the new `ldu.global` and typed `selp` generator paths with
the earlier known integer families suppressed:

```text
divergences/active-20260518-222242-ldu-selp-known-suppressed/div-1779143049-18b0c91e896f7b83
```

The original hit did not depend on `ldu.global` or typed `selp`. Reduction
showed the trigger is a scalar 16-bit `max.s16` through `.b16` scratch
registers feeding a predicate that guards a 16-bit-source `mul.wide.u16`.

## Scalar Trace

For `reduced.ptx`, with `in_n = 32`:

```text
h0 = cvt.s16.s32 n         # 32
h1 = cvt.s16.s32 0
h2 = max.s16 h0, h1        # 32
r3 = cvt.s32.s16 h2        # 32
p0 = (0 == r3)             # false
h0 = cvt.u16.u32 input
h1 = cvt.u16.u32 38971
@!p0 r0 = mul.wide.u16 h0, h1
```

The correct output is `(input & 0xffff) * 38971`. Affected optimized ptxas
keeps the pre-multiply value `0x20` instead.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-18 with:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

For continued fuzzing past this family, use
`DIV_DISABLE_PREDICATED_SUBWORD_WIDE=1`.
