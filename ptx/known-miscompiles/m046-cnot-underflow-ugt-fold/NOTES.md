# m046-cnot-underflow-ugt-fold

Found while continuing structured control-flow fuzzing after suppressing the
known families through `m045-brev-branch-fold`.

The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-after-m045-XfoAHddu/div-1778986120-18b03a283cd87617
```

The automated reducer shrank the original 359-line program to 91 lines. Manual
cleanup showed the control flow and `slct.b32.s32` in that reduction were not
needed; the core issue is a straight-line `cnot.b32` value feeding wrapped
subtraction before an unsigned comparison.

## Scalar Trace

The kernel is launched with 32 threads, so `%tid.x` is `0..31`.

```text
%r1 = 32 & %tid.x = 0
%r2 = cnot(%r1) = 1
%r3 = %tid.x + 1
%r4 = %tid.x - %r3 = 0xffffffff
%p0 = 0 > %r4 = false, using unsigned comparison
%r5 = selp(52761, 0, %p0) = 0
```

`ptxas -O0` stores `0x00000000` in output slot 0 for every lane, while
optimized ptxas stores `0x0000ce19` (`52761`).

This is likely the same broad root cause as `m032-cnot-neg-ugt-fold`: a
`cnot`-derived value participates in wrapped arithmetic before an unsigned
comparison predicate, and the optimized fold selects the wrong arm. A queued
duplicate found before enabling `DIV_DISABLE_CNOT` reduced to the same pattern
with `setp.ge.u32`.

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

For continued fuzzing past this family, use `DIV_DISABLE_CNOT=1`.
