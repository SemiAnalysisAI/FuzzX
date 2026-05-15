# m018-subc-cnot-shift-borrow-fold

Found after adding `sub.cc.u32`/`subc.u32` pair generation and increasing the
structured program shape with earlier known triggers disabled:

```text
PTXAS=/tmp/cuda-13.2.1-ptxas/extract/usr/local/cuda-13.2/bin/ptxas
DIV_OUT_DIR=/tmp/fuzzx-structured-xl-knownflags-noaddc-nos32slct-20k
DIV_STRUCTURED_CONTROL_FLOW=1
DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 DIV_DISABLE_MULHI=1
DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 DIV_DISABLE_NEG=1
DIV_DISABLE_ABS=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1
DIV_DISABLE_SIGNED_SHR=1 DIV_DISABLE_BFIND=1
DIV_DISABLE_I32_BOUNDARY_IMMS=1 DIV_DISABLE_SET=1
DIV_DISABLE_VSUB4=1 DIV_DISABLE_S32_SLCT=1 DIV_DISABLE_ADDC=1
DIV_MIN_BLOCKS=8 DIV_MAX_BLOCKS=40 DIV_MAX_INSTS_PER_BLOCK=18
DIV_WORKING_REGS=20 DIV_MAX_LOOP_ITERS=80 DIV_MAX_IMMEDIATE=4096
DIV_PROGRAM_BYTES=12288
seed 0x18afa156cb8648fd
```

The original saved fuzzer program was in
`/tmp/fuzzx-structured-xl-knownflags-noaddc-nos32slct-20k/div-1778817859-18afa156cb8648fd`
on the machine where this was reduced.

Manual reduction collapsed the test to a `cnot`-derived zero, a shift, one
borrow-producing subtract, and one borrow-consuming subtract:

```text
cnot.b32   r5, in_n;
shl.b32    r9, r5, 12;
sub.cc.u32 r4, 3285, r9;
subc.u32   r9, 939, 433;
```

Replacing the `cnot` with `sub.u32 r5, in_n, in_n` or `and.b32 r5, in_n, 0`
made the reduced test stop diverging, so this is specifically a `cnot`-derived
zero feeding the shifted `sub.cc` source.

## Correct Scalar Trace

The standalone reproducer passes `in_n = 32`, so:

```text
r5 = cnot.b32(32)        = 0
r9 = r5 << 12            = 0
r4 = sub.cc.u32 3285, r9 = 3285, borrow-out 0
r9 = subc.u32 939, 433   = 939 - (433 + borrow-in 0) = 506
```

The correct output is therefore:

```text
0x000001fa
```

`ptxas -O0` matches that trace. `ptxas -O1`, `ptxas -O2`, and `ptxas -O3`
store `0x000001f9`.

Standalone C++ bug-report repro: `repro_ptxas_subc_cnot_shift_o1.cpp`. It
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

At `-O0`, ptxas keeps the dataflow through `cnot` and `shl`, then uses the
real borrow-out from `3285 - 0` as the borrow-in to `subc`:

```text
ISETP.EQ.U32.AND P0, PT, R0, RZ, PT ;
SEL               R0, RZ, 0xffffffff, !P0 ;
IADD3             R0, PT, PT, RZ, -R0, RZ ;
SHF.L.U32         R0, R0, 0xc, RZ ;
IADD3             RZ, P0, PT, -R0, 0xcd5, RZ ;
LOP3.LUT          R0, RZ, 0x1b1, RZ, 0x33, !PT ;
IADD3.X           R0, PT, PT, R0, 0x3ab, RZ, P0, !PT ;
STG.E             desc[UR4][R2.64], R0 ;
```

For `in_n = 32`, `cnot` produces zero and `3285 - 0` does not borrow, so the
final `IADD3.X` stores `0x1fa`.

At `-O1` and above, ptxas folds the sequence into a shorter predicate form:

```text
ISETP.EQ.U32.AND P0, PT, RZ, UR6, PT ;
SEL               R0, RZ, 0xffffffff, !P0 ;
IADD3.X           R5, PT, PT, R5, -0x1b2, RZ, P0, !PT ;
STG.E             desc[UR4][R2.64], R5 ;
```

For `in_n = 32`, that stores `0x1f9`, which is exactly what `subc.u32 939,
433` would produce with an incorrect borrow-in of 1. The optimized compiler
has therefore folded the borrow-out of `sub.cc.u32 3285, (cnot(in_n) << 12)`
to 1 even though the shifted source is zero and the correct borrow-out is 0.
This is related to, but distinct from, m017's `addc` carry-folding bug.
