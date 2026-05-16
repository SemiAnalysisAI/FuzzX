# m042-vsub4-else-ifconvert-fold

Found while continuing structured control-flow fuzzing after suppressing the
known `not.b32`, `xor.b32`, `or.b32`, `bfind.u32`, `mul24.*`, `mul.hi.*`,
`mul.wide.*`, `bfi.b32`, `bmsk.clamp.b32`, `dp2a.*`, and `mad24.*` families
from earlier runs.

The original saved fuzzer program was:

```text
/tmp/fuzzx-bg-after-m041-20260515T224930Z/div-1778885399-18afded44bd77593
```

The minimized PTX in `reduced.ptx` keeps the fuzzer input-buffer load. Only
`%tid.x == 2` executes the else arm and differs between optimization levels.

## Scalar Trace

For `%tid.x == 2`:

```text
%r15 = in_n = 32
%r17 = %tid.x = 2
%r9  = %tid.x = 2
%p5  = 2 != %r17 = false
```

Because `%p5` is false, execution takes `structured_if_1_else`:

```text
%r4 = vsub4.u32.u32.u32 %r15, %r17, %r9
%r1 = %r4 * in[2] + 46474
```

With the saved input, `in[2] = 0x882a34e1`. `ptxas -O0` stores
`0xf4f2e7e8` for lane 2, while optimized ptxas stores `0x14fdd868`.
All other lanes keep their initialized `%r1 = %tid.x` value.

Forcing either branch removes the divergence. Replacing the `vsub4` result
with the scalar constant also removes the divergence, while replacing the
`mad.lo` with an equivalent add does not. This points to an if-conversion or
control-flow fold around the `vsub4` else-arm value, rather than a standalone
multiply/add fold.

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

For continued fuzzing past this family, use `DIV_DISABLE_VSUB4=1`; the
reduced testcase specifically needs `vsub4.u32.u32.u32`.
