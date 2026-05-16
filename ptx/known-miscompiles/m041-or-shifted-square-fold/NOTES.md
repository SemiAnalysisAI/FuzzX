# m041-or-shifted-square-fold

Found while continuing structured control-flow fuzzing after suppressing the
known `not.b32`, `xor.b32`, `bfind.u32`, `mul24.*`, `mul.hi.*`,
`mul.wide.*`, `bfi.b32`, `bmsk.clamp.b32`, `dp2a.*`, and `mad24.*` families
from earlier runs.

The original saved fuzzer program was:

```text
/tmp/fuzzx-bg-after-m040-20260515T222208Z/div-1778883798-18afdd55cede0677
```

The minimized PTX in `reduced.ptx` keeps the fuzzer input-buffer load, but the
reduced value chain makes the final squared value's low 32 bits zero for all
threads and inputs.

## Scalar Trace

The key chain is:

```text
%r9  = 0x02000000
%r0  = %r2 * %r9
%r1  = 0xff00ff00 & %tid.x
%r3  = %r0 * 16 + %r1
%r1  = 620912 - %r3
%r19 = 0x00080000 * %r1
%r0  = %r19 * %r19
%r0  = %r0 | 0x0000d7cc
```

Because `%r19` is always a multiple of `2^19`, `%r19 * %r19` is a multiple of
`2^38` and its low 32 bits are zero. The final `or.b32` should therefore
store `0x0000d7cc` for every thread. Optimized ptxas stores `0x000097cc`,
clearing bit `0x4000`.

The generated optimized SASS collapses the final value to an `IMAD` immediate
form using `0x97cc`, which points to an integer-combiner fold of the final
`or.b32` after the shifted square.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-15 with CUDA Toolkit 13.2 Update 1 ptxas, the
latest NVIDIA CUDA Toolkit listed on NVIDIA's CUDA Toolkit Archive on
2026-05-15:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_OR=1`; the reduced
testcase specifically needs the final generated `or.b32`.
