# m016-slct-s32-immediate-fold

Found after expanding structured-control-flow fuzzing with earlier known
triggers disabled:

```text
PTXAS=/tmp/cuda-13.2.1-ptxas/extract/usr/local/cuda-13.2/bin/ptxas
DIV_OUT_DIR=/tmp/ptx-fuzz-structured-large-knownflags-expanded-noabs
DIV_STRUCTURED_CONTROL_FLOW=1
DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 DIV_DISABLE_MULHI=1
DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 DIV_DISABLE_NEG=1
DIV_DISABLE_ABS=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1
DIV_DISABLE_SIGNED_SHR=1 DIV_DISABLE_BFIND=1
DIV_DISABLE_I32_BOUNDARY_IMMS=1 DIV_DISABLE_SET=1
DIV_DISABLE_VSUB4=1
DIV_MIN_BLOCKS=6 DIV_MAX_BLOCKS=32 DIV_MAX_INSTS_PER_BLOCK=14
DIV_WORKING_REGS=16 DIV_MAX_LOOP_ITERS=64 DIV_MAX_IMMEDIATE=2048
DIV_PROGRAM_BYTES=8192
seed 0x18af9dcb214d0d58
```

The original saved fuzzer program was in
`/tmp/ptx-fuzz-structured-large-knownflags-expanded-noabs/div-1778814334-18af9dcb214d0d58`
on the machine where this was reduced.

Manual reduction collapsed the test to one `slct.s32.s32` with an immediate
positive selector:

```text
slct.s32.s32 %r0, 1, 12, 2142016040;
```

## Correct Scalar Trace

The PTX ISA specifies `slct.dtype.s32 d, a, b, c` as:

```text
d = (c >= 0) ? a : b
```

Here `c = 2142016040 = 0x7fac9228`, whose sign bit is clear, so `c` is a
positive signed 32-bit integer. The correct selected operand is therefore
`a = 1`, and the correct output is:

```text
0x00000001
```

`ptxas -O0` matches that trace. `ptxas -O1`, `ptxas -O2`, and `ptxas -O3`
store `0x0000000c`.

Standalone C++ bug-report repro: `repro_ptxas_slct_s32_immediate_o1.cpp`. It
embeds the reduced PTX, compiles it with `ptxas -O0`, `ptxas -O1`, `ptxas
-O2`, and `ptxas -O3`, launches one CUDA thread through the CUDA Driver API,
and returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-15 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). NVIDIA's CUDA Toolkit archive lists
CUDA Toolkit 13.2.1 as the latest release, and NVIDIA's CUDA 13.2 Update 1
release notes list CUDA Application Compiler version `13.2.78`.

Sources checked:

* https://developer.nvidia.com/cuda-toolkit-archive
* https://docs.nvidia.com/cuda/cuda-toolkit-release-notes/index.html
* https://docs.nvidia.com/cuda/parallel-thread-execution/index.html

SASS below was decoded with matching CUDA 13.2.1 `nvdisasm` V13.2.78, build
`cuda_13.2.r13.2/compiler.37668154_0`.

## SASS Root Cause

At `-O0`, ptxas emits the sign test and select. The predicate computes
`0 <= 0x7fac9228`, which is true, so `SEL` chooses the first source operand
`1`:

```text
ISETP.LE.AND P0, PT, RZ, 0x7fac9228, PT ;
MOV          R0, 0x1 ;
SEL          R0, R0, 0xc, P0 ;
STG.E        desc[UR4][R2.64], R0 ;
```

At `-O1` and above, ptxas constant-folds the `slct` away and stores
`0x0000000c` instead. `nvdisasm` prints the immediate materialization as an
`HFMA2` instruction; the half immediate shown is the bit pattern `0x000c`:

```text
HFMA2        R5, -RZ, RZ, 0, 7.152557373046875e-07 ;
STG.E        desc[UR4][R2.64], R5 ;
```

The optimized compiler has therefore folded `slct.s32.s32` with a positive
immediate selector to the second operand instead of the first operand. This is
distinct from the earlier loop-predicate, video, and empty-loop/folding bugs;
no loop or input data is needed.
