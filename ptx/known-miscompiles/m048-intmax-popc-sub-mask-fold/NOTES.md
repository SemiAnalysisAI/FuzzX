# m048-intmax-popc-sub-mask-fold

Found while continuing structured control-flow fuzzing after suppressing the
known families through `m047-selp-ge-zero-branch-fold`.

The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-postknown-hugeimm-structured-iECrcQBw/div-1779013868-18b053846da0b43b
```

The automated reducer shrank the original 2,644-line program to 180 lines.
Manual cleanup showed that a straight-line scalar version and a simple
structured branch version both compile correctly; the remaining trigger still
needs structured-branch context around the value chain.

## Scalar Trace

The kernel is launched with 32 threads, so `%tid.x` is `0..31`. The divergent
lanes are the lanes where the loaded input is `<= 2488671102`, causing the
then arm to compute `%r23` and `%r26`:

```text
%r21 = popc(0x7a5e1ae0) = 16
%r26 = popc(%r21) = 1
%r1  = 2147483646 + %r26 = 0x7fffffff
%r4  = 3046743225 - %r1 = 0x35999cba
%r36 = input + 32
%r23 = %r4 & %r36
%r0  = %r23 + %r26
```

`ptxas -O0` follows that trace. Optimized ptxas produces results that are
`2` lower for the eight affected lanes. The optimized SASS uses `0x35999cb8`
as the `and` mask instead of the correct `0x35999cba`, as if the
`0x7fffffff` subtract input had been treated like `0x80000001`.

This is likely related to the signed-boundary fold in
`m012-empty-loop-intmax-sub`, but this testcase does not require loops. The
extra structured-branch context is still part of the trigger in the reduced
PTX.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-17 with CUDA Toolkit 13.2.1 nvcc/ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_I32_BOUNDARY_IMMS=1`.
This finding showed that the suppressor needed to cover values near the
signed 32-bit boundary, not only literal `0x7fffffff` and `0x80000000`.
