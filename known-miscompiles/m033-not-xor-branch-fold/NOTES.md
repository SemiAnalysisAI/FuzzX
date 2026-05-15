# m033-not-xor-branch-fold

Found while fuzzing structured control flow with `not.b32` and `cnot.b32`
enabled, `clz.b32` and `neg.s32` disabled, and the earlier boundary-immediate
and LOP/min/max suppressors enabled:

```text
DIV_CONTROL_FLOW_MODE=structured-if
DIV_MIN_BLOCKS=8
DIV_MAX_BLOCKS=20
DIV_MIN_INSTS_PER_BLOCK=10
DIV_MAX_INSTS_PER_BLOCK=28
DIV_PROGRAM_BYTES=16384
DIV_WORKING_REGS=20
DIV_MAX_IMMEDIATE=65536
DIV_DISABLE_ARBITRARY_LOOPS=1
DIV_DISABLE_STRUCTURED_LOOPS=1
DIV_DISABLE_LOP3=1
DIV_DISABLE_MINMAX=1
DIV_DISABLE_SHL=1
DIV_DISABLE_PRMT=1
DIV_DISABLE_SIGNED_CMP=1
DIV_DISABLE_FUNNEL=1
DIV_DISABLE_ABS=1
DIV_DISABLE_SIGNED_SHR=1
DIV_DISABLE_ADDC=1
DIV_DISABLE_SUBC=1
DIV_DISABLE_SET=1
DIV_DISABLE_S32_SLCT=1
DIV_DISABLE_CLZ=1
DIV_DISABLE_NEG=1
DIV_DISABLE_I32_BOUNDARY_IMMS=1
seed 0x18afbc122031be23
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-structured-if-xlarge-imm65536-knownflags-not-noclz-cnot-noneg-sub-noboundary-200k/div-1778847323-18afbc122031be23
```

The minimized PTX in `reduced.ptx` does not read the input buffer or `in_n`.
The dummy `in_ptr` and `in_n` parameters are kept only to match the fuzzer ABI.

## Correct Scalar Trace

Only `%tid.x == 20` reaches the store:

```text
%r20 = %tid.x = 20
%p0 = setp.ne.u32 %r20, 20 = false
%r13 = not.b32 %r20 = ~20 = 0xffffffeb
%r4 = xor.b32 %r13, 0x9033a9b3
    = 0xffffffeb ^ 0x9033a9b3
    = 0x6fcc5658
store %r4 to out[20][3]
```

All other lanes branch directly to `exit` and do not write. The harness zeros
the output buffer before launch, so the only nonzero word should be
`out[20][3] = 0x6fcc5658`.

`ptxas -O0` stores `0x6fcc5658`. With affected ptxas versions, `ptxas -O1`,
`-O2`, and `-O3` store `0x9033a9a7`, which is exactly:

```text
20 ^ 0x9033a9b3 = 0x9033a9a7
```

That is, optimized ptxas behaves as if the `not.b32` was dropped from the
value used by the `xor.b32` in the `%tid.x == 20` branch.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS Root Cause

At `-O0`, ptxas preserves the `not` before the `xor`:

```text
LOP3.LUT             R8, RZ, R0, RZ, 0x33, !PT ;  // ~tid.x
ISETP.NE.U32.AND     P0, PT, R0, 0x14, PT ;
@P0 BRA              ...
LOP3.LUT             R8, R8, 0x9033a9b3, RZ, 0x3c, !PT ;
STG.E                ..., R8 ;
```

At `-O1` and above, ptxas specializes the `%tid.x == 20` path but folds the
wrong value into the store:

```text
ISETP.NE.U32.AND     P0, PT, R7, 0x14, PT ;
@!P0 IMAD.MOV.U32    R0, RZ, RZ, -0x6fcc5659 ;
STG.E                ..., R0 ;
```

`-0x6fcc5659` is `0x9033a9a7`, which is `20 ^ 0x9033a9b3`. The correct folded
constant is `(~20) ^ 0x9033a9b3 = 0x6fcc5658`.
