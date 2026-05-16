# m035-xor-not-predicate-fold

Found while fuzzing structured control flow after suppressing the `not.b32`
and `bfind.u32` root causes from m033/m034:

```text
DIV_STRUCTURED_CONTROL_FLOW=1
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
DIV_DISABLE_NEG=1
DIV_DISABLE_NOT=1
DIV_DISABLE_BFIND=1
DIV_DISABLE_I32_BOUNDARY_IMMS=1
seed 0x18afc142d1aab235
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-structured-if-xlarge-imm65536-knownflags-nonot-nobfind-clz-cnot-noneg-sub-noboundary-200k/div-1778852924-18afc142d1aab235
```

The minimized PTX in `reduced.ptx` does not read the input buffer or `in_n`.
The dummy `in_ptr` and `in_n` parameters are kept only to match the fuzzer ABI.

## Correct Scalar Trace

For `%tid.x == 4`, the branch is not taken:

```text
%r18 = 0
%r1  = xor.b32 %tid.x, 0xffffffff = 0xfffffffb
%p1  = setp.ge.u32 %r1, 32 = true
%r18 = selp.b32 0x04000000, 0x0000588a, %p1 = 0x04000000
%r2  = shr.u32 %r18, 24 = 4
store %r2 to out[4][2]
```

For `%tid.x != 4`, the branch skips the xor/compare/select, leaving `%r18 = 0`,
so those lanes store `0`.

`ptxas -O0` stores `4` for lane 4. With affected ptxas versions, `ptxas -O1`,
`-O2`, and `-O3` store `0`, as if `xor.b32 %tid.x, 0xffffffff` had been
dropped from the unsigned compare and the false arm of the `selp` had been
selected.

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

At `-O0`, ptxas preserves the PTX dataflow:

```text
ISETP.NE.U32.AND    P0, PT, R7, 0x4, PT ;
@P0 BRA             ...
LOP3.LUT            R17, R7, 0xffffffff, RZ, 0x3c, !PT ;
ISETP.GE.U32.AND    P0, PT, R17, 0x20, PT ;
SEL                 R17, 0x4000000, 0x588a, P0 ;
SHF.R.U32.HI        R17, RZ, 0x18, R17 ;
STG.E               ..., R17 ;
```

At `-O1` and above, ptxas folds the lane-4 path to the wrong select arm:

```text
ISETP.NE.U32.AND    P0, PT, R5, 0x4, PT ;
@!P0 MOV            R0, 0x588a ;
SHF.R.U32.HI        R5, RZ, 0x18, R0 ;
STG.E               ..., R5 ;
```

For `%tid.x == 4`, this stores `0x0000588a >> 24 = 0`. The PTX requires
`(4 ^ 0xffffffff) >= 32`, which is true, so the selected value is
`0x04000000` and the stored result is `4`.

This is the same root-cause family as m033: ptxas mishandles a bitwise-not-like
value feeding a predicate. For further fuzzing, treat `xor.b32` by
`0xffffffff` as a not idiom when suppressing this class.
