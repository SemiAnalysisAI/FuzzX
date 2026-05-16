# m039-else-redefinition-fold

Found while continuing structured control-flow fuzzing after suppressing the
known `not.b32`, `bfind.u32`, `mul24.*`, `mul.hi.*`, `bfi.b32`,
`bmsk.clamp.b32`, `dp2a.*`, and `mad24.*` families from earlier runs.

The original saved fuzzer program was:

```text
/tmp/fuzzx-bg-after-m038-20260515T205530Z/div-1778880216-18afd8992784c231
```

The minimized PTX in `reduced.ptx` does not read the input buffer. The dummy
`in_ptr` parameter is kept only to match the fuzzer ABI shape used by the
standalone harness.

## Scalar Trace

```text
%r18 = %tid.x - %tid.x = 0
%p0  = 0 != %r18 = false
```

Because `%p0` is false, execution branches to `L_else`:

```text
%r12 = %r18 ^ 0xffffffff = 0xffffffff
%r18 = 65117 * %r12 = 0xffff01a3
```

`ptxas -O0` stores `0xffff01a3` for every thread. Optimized ptxas stores
`0x00000000`, apparently folding the control-flow merge to the pre-branch
`%r18 = 0` value and dropping the executed else-path redefinition.

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

For continued fuzzing past this family, use `DIV_DISABLE_XOR=1`; this reduced
testcase requires the generated `xor.b32` in the executed else arm.
