# m043-shr-sub-branch-fold

Found while continuing structured control-flow fuzzing after suppressing the
known families through `m042-vsub4-else-ifconvert-fold`.

The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-after-m042-20260517T002334Z/div-1778978195-18b032888b57cced
```

The minimized PTX in `reduced.ptx` no longer depends on the input buffer. Only
`%tid.x == 0` takes the else arm and differs between optimization levels.

## Scalar Trace

For `%tid.x == 0`:

```text
%r1 = %tid.x = 0
%r2 = 0xffffffff
%r3 = %r1 + %r2 = 0xffffffff
%p0 = %r1 != 0 = false
```

Because `%p0` is false, execution takes `else_path`:

```text
%r4 = %r2 >> 1 = 0x7fffffff
%r5 = %r2 - %r3 = 0
%r6 = %r1 - %r4 = 0x80000001
%r3 = %r6 >> 8 = 0x00800000
```

`ptxas -O0` stores `0x00800000` in output slot 3 for lane 0, while optimized
ptxas stores `0x007fffff`. All other lanes take `then_path` and keep the
pre-branch `%r3 = %tid.x - 1` value.

During probing, the original `bfe`-derived expression could be simplified away;
the reduced testcase is a pure `shr.u32` / `sub.u32` / `mad.lo.u32` / `and.b32`
chain. Removing the live `mad.lo.u32` / `and.b32` value chain or the store at
output slot 0 removes the divergence. This looks like a branch/liveness-
sensitive fold around unsigned right shift after wrapped subtraction; the
optimized result is consistent with losing the wrapped high bit before the
shift.

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

For continued fuzzing past this family, use `DIV_DISABLE_SHR=1`; the reduced
testcase specifically needs unsigned `shr.u32`.
