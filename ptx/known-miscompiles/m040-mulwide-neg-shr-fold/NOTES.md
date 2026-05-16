# m040-mulwide-neg-shr-fold

Found while continuing structured control-flow fuzzing after suppressing the
known `not.b32`, `xor.b32`, `bfind.u32`, `mul24.*`, `mul.hi.*`, `bfi.b32`,
`bmsk.clamp.b32`, `dp2a.*`, and `mad24.*` families from earlier runs.

The original saved fuzzer program was:

```text
/tmp/fuzzx-bg-after-m039-xor-disabled-20260515T220454Z/div-1778882848-18afdc64b28b6c5d
```

The minimized PTX in `reduced.ptx` does not read the input buffer. The dummy
`in_ptr` parameter is kept only to match the fuzzer ABI shape used by the
standalone harness.

## Scalar Trace

For each thread:

```text
%r0 = %tid.x
%rd2 = mul.wide.u32 %r0, %r0
%r1 = low32(%rd2)
%r3 = 0 - %r1
%r4 = %r3 >> 17
%r5 = %r4 + 1
```

For `%tid.x = 0`, both opt levels store `0x00000001`. For `%tid.x = 1..31`,
the correct low product is small and nonzero, so `0 - %r1` wraps into the high
unsigned range and logical-shifting right by 17 yields `0x00007fff`; after the
final add, `ptxas -O0` stores `0x00008000`.

Optimized ptxas stores `0x00000001` for every thread, apparently dropping the
`0x00007fff` contribution from the wrapped negated low product.

Replacing the root `mul.wide` with `mul.lo.u32` removes the divergence. Both
`mul.wide.u32` and `mul.wide.s32` reproduced in the reduced shape. This looks
like a low-word fold bug for `mul.wide` feeding `0 - x`, logical shift, and
add-one.

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

For continued fuzzing past this family, use `DIV_DISABLE_MUL_WIDE=1`.
