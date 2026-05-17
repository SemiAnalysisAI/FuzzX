# m047-selp-ge-zero-branch-fold

Found while continuing structured control-flow fuzzing after suppressing the
known families through `m046-cnot-underflow-ugt-fold`.

The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-after-m046-8wrcAWkG/div-1778990337-18b03d79b2a8c54e
```

The automated reducer shrank the original 417-line program to 98 lines. Manual
cleanup showed the core issue is `selp.b32` materializing `0xffffffff` before
an unsigned comparison and branch.

## Scalar Trace

The kernel is launched with 32 threads, so `%tid.x` is `0..31`.

```text
%p0 = (32 != %tid.x) = true
%r1 = selp(0xffffffff, 0, %p0) = 0xffffffff
%p1 = (%r1 >= 0) = true, using unsigned comparison
```

The `then_path` arm is therefore always taken and `ptxas -O0` stores
`0x0000001b` in output slot 2 for every lane. Optimized ptxas skips the arm and
stores `0x00000000`.

This is related to the materialized-boolean fold in `m013-set-true-cmp-one`,
but the exact trigger is different: replacing `selp.b32` with a direct
`mov.u32 0xffffffff` removes the divergence, and replacing the second branch
with a direct branch on `%p0` also removes it.

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

For continued fuzzing past this family, use `DIV_DISABLE_SELP=1`.
