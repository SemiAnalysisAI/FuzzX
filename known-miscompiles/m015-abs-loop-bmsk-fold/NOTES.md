# m015-abs-loop-bmsk-fold

Found after adding video min/max instruction coverage while fuzzing structured
control flow with earlier known triggers disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 \
DIV_DISABLE_NEG=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1 \
DIV_DISABLE_SIGNED_SHR=1 DIV_DISABLE_BFIND=1 \
DIV_DISABLE_I32_BOUNDARY_IMMS=1 DIV_DISABLE_SET=1 DIV_DISABLE_VSUB4=1 \
DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 \
DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af974bc0f86648
```

The original saved fuzzer program was in
`/tmp/ptx-fuzz-structured-expanded-knownflags-video-minmax-novsub4/div-1778806743-18af974bc0f86648`
on the machine where this was reduced.

The original fuzzer program had no video instructions, so this was not caused
by the newly-added video min/max operations. Manual reduction left one
three-trip loop, one `abs.s32`, one `bmsk.clamp.b32`, and one output word.

## Correct scalar trace

The standalone reproducer launches one thread, so `%tid.x = 0`. The reduced
PTX starts with:

```text
r1 = 3
r7 = tid.x = 0
r10 = 0
```

The loop is a do-while loop that executes exactly three times:

```text
r1  = r1 - 1
r5  = r10 - 1
r10 = r7 >> 1
r2  = abs.s32(r5)
r7  = r10 & 1
repeat while r1 != 0
```

For `%tid.x = 0`, every iteration has `r10 = 0`, so `r5 = -1` and
`abs.s32(-1) = 1`. After the third iteration:

```text
r2 = 1
```

The post-loop code computes:

```text
bmsk.clamp.b32 r3, 13, 9  => ((1 << 9) - 1) << 13 = 0x003fe000
r3 >> 8                   => 0x00003fe0
r6 = r2 + r3              => 1 + 0x3fe0 = 0x00003fe1
```

So the correct output is:

```text
0x00003fe1
```

`ptxas -O0` and `ptxas -O1` match that trace. `ptxas -O2` and `ptxas -O3`
store `0x00003fdf`.

Standalone C++ bug-report repro: `repro_ptxas_abs_loop_bmsk_o2.cpp`. It embeds
the reduced PTX, compiles it with `ptxas -O0`, `ptxas -O1`, `ptxas -O2`, and
`ptxas -O3`, launches one CUDA thread through the CUDA Driver API, and returns
1 when the bug is reproduced.

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

SASS below was decoded with matching CUDA 13.2.1 `nvdisasm` V13.2.78, build
`cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O0`, ptxas keeps the source loop. The loop computes `abs.s32(-1) = 1`
into the live-out register and then adds it to `bmsk(13, 9) >> 8`:

```text
IADD3           R3, PT, PT, R3, -0x1, RZ ;
SHF.R.U32.HI    R2, RZ, 0x1, R2 ;
IABS            R3, R3 ;
LOP3.LUT        R7, R2, 0x1, RZ, 0xc0, !PT ;
@P0 BRA         loop ;
BMSK            R0, R0, 0x9 ;
SHF.R.U32.HI    R0, RZ, 0x8, R0 ;
IADD3           R0, PT, PT, R6, R0, RZ ;
STG.E           desc[UR4][R2.64], R0 ;
```

At `-O2`, ptxas deletes the loop and stores a folded constant:

```text
IMAD.MOV.U32    R0, RZ, RZ, 0xd ;
BMSK            R0, R0, 0x9 ;
LEA.HI          R5, R0, 0xffffffff, RZ, 0x18 ;
STG.E           desc[UR4][R2.64], R5 ;
```

For `BMSK(13, 9) = 0x003fe000`, the optimized `LEA.HI` path stores
`0x00003fdf`, which is equivalent to `(0x003fe000 >> 8) - 1`. The correct
source value is `(0x003fe000 >> 8) + abs.s32(-1) = 0x00003fe1`.

The optimized compiler has therefore used the pre-`abs.s32` value `-1` as the
loop live-out feeding the post-loop add, even though `abs.s32` itself makes the
live-out value `+1`. This is distinct from the earlier uniform loop-predicate
bug, from the `vsub4` biased-intermediate bug, and from the prior empty-loop
folding bugs.
