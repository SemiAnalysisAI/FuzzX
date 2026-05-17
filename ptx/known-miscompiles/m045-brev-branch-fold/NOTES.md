# m045-brev-branch-fold

Found while continuing structured control-flow fuzzing after suppressing the
known families through `m044-mul-lo-square-fold`.

The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-after-m044-rVnc5hYa/div-1778983878-18b038391834cbdb
```

The automated reducer shrank the original 389-line program to 105 lines. Manual
cleanup showed the core issue is a branch join around a `brev.b32` value.

## Scalar Trace

The kernel is launched with 32 threads, so `%tid.x == 32` is false for every
lane. The executed path is:

```text
%r1 = 0xfffffffe
%r2 = brev(%r1) = 0x7fffffff
%r3 = 32 - %r2 = 0x80000021
%r4 = %r1 + %r3 = 0x8000001f
```

`ptxas -O0` stores `0x8000001f` in output slot 0 for every lane, while
optimized ptxas stores `0x8000001d`.

Straight-line `brev.b32` did not reproduce by itself during minimization. The
wrong fold needs the control-flow join where the same register is conditionally
redefined on the untaken path.

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

For continued fuzzing past this family, use `DIV_DISABLE_BREV=1`.
