# m017-addc-shift-carry-fold

Found immediately after adding `add.cc.u32`/`addc.u32` pair generation while
fuzzing structured control flow with earlier known triggers disabled:

```text
PTXAS=/tmp/cuda-13.2.1-ptxas/extract/usr/local/cuda-13.2/bin/ptxas
DIV_OUT_DIR=/tmp/ptx-fuzz-structured-large-knownflags-expanded-noabs-nos32slct-addc-30k
DIV_STRUCTURED_CONTROL_FLOW=1
DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 DIV_DISABLE_MULHI=1
DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 DIV_DISABLE_NEG=1
DIV_DISABLE_ABS=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1
DIV_DISABLE_SIGNED_SHR=1 DIV_DISABLE_BFIND=1
DIV_DISABLE_I32_BOUNDARY_IMMS=1 DIV_DISABLE_SET=1
DIV_DISABLE_VSUB4=1 DIV_DISABLE_S32_SLCT=1
DIV_MIN_BLOCKS=6 DIV_MAX_BLOCKS=32 DIV_MAX_INSTS_PER_BLOCK=14
DIV_WORKING_REGS=16 DIV_MAX_LOOP_ITERS=64 DIV_MAX_IMMEDIATE=2048
DIV_PROGRAM_BYTES=8192
seed 0x18af9fde50baeb5a
```

The original saved fuzzer program was in
`/tmp/ptx-fuzz-structured-large-knownflags-expanded-noabs-nos32slct-addc-30k/div-1778816173-18af9fde50baeb5a`
on the machine where this was reduced.

Manual reduction collapsed the test to two shifts, one carry-producing add,
and one carry-consuming add:

```text
shl.b32    r9, in_n, 1;
shl.b32    r9, r9, 31;
add.cc.u32 r11, r9, 536;
addc.u32   r9, 64, 0;
```

## Correct Scalar Trace

The standalone reproducer passes `in_n = 32`, so:

```text
r9  = 32 << 1            = 0x00000040
r9  = r9 << 31           = 0x00000000
r11 = add.cc.u32 r9, 536 = 0x00000218, carry-out 0
r9  = addc.u32 64, 0     = 0x00000040 + carry-in 0
```

The correct output is therefore:

```text
0x00000040
```

`ptxas -O0` matches that trace. `ptxas -O1`, `ptxas -O2`, and `ptxas -O3`
store `0x00000041`.

Standalone C++ bug-report repro: `repro_ptxas_addc_shift_carry_o1.cpp`. It
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

At `-O0`, ptxas keeps the two shifts and uses the real carry-out from the
`add.cc` lowering as the carry-in to `addc`:

```text
SHF.L.U32 R0, R0, 0x1, RZ ;
SHF.L.U32 R0, R0, 0x1f, RZ ;
IADD3     RZ, P0, PT, R0, 0x218, RZ ;
IADD3.X   R0, PT, PT, RZ, 0x40, RZ, P0, !PT ;
STG.E     desc[UR4][R2.64], R0 ;
```

For `in_n = 32`, the shifted value is zero, so `0 + 0x218` does not carry.
`P0` is therefore false and `IADD3.X` stores `0x40`.

At `-O1` and above, ptxas folds the shifts and the `add.cc`, but then emits an
`IADD3.X` with a forced carry-in:

```text
IADD3.X   R5, PT, PT, RZ, 0x40, RZ, PT, !PT ;
STG.E     desc[UR4][R2.64], R5 ;
```

That stores `0x41`. The optimized compiler has therefore folded the carry-out
of `add.cc.u32 ((in_n << 1) << 31), 536` to 1, even though the source value is
zero for `in_n = 32` and the correct carry-out is 0. This is distinct from the
previous `slct.s32.s32` immediate-folding bug.
