# m044-mul-lo-square-fold

Found while continuing structured control-flow fuzzing after suppressing the
known families through `m043-shr-sub-branch-fold`.

The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-after-m043-20260517T011357Z/div-1778980758-18b0354866f6a831
```

The minimized PTX in `reduced.ptx` is straight-line and does not depend on the
input buffer.

## Scalar Trace

For every lane:

```text
%r0 = %tid.x
%r1 = 4 * %r0 + 2
%r1 = %r1 * 65536
```

At this point `%r1` has at least 17 trailing zero bits. Squaring it with
`mul.lo.u32` must therefore produce zero in the low 32 bits:

```text
%r1 = (%r1 * %r1) & 0xffffffff = 0
%r1 = %r1 * 536870912 = 0
```

`ptxas -O0` stores `0x00000000` in output slot 1 for every lane, while
optimized ptxas stores `0x80000000`.

This is likely the same broad root cause as `m041-or-shifted-square-fold`.
`m041` was originally suppressed by disabling `or.b32`, but this reproducer
shows the problematic square/low-bit reasoning survives without `or.b32`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-17 with CUDA Toolkit 13.2.1 ptxas, the latest
NVIDIA CUDA Toolkit available locally and checked against NVIDIA's CUDA
Toolkit Archive at task time:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_MUL_LO=1`; the
reduced testcase specifically needs `mad.lo.u32` / `mul.lo.u32`.
