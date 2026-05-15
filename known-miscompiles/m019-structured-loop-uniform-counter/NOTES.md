# m019-structured-loop-uniform-counter

Found while fuzzing structured control flow with earlier known instruction
triggers disabled and with `mul.wide` / 64-bit ALU coverage enabled:

```text
PTXAS=/tmp/cuda-13.2.1-ptxas/extract/usr/local/cuda-13.2/bin/ptxas
DIV_OUT_DIR=/tmp/ptx-fuzz-structured-deep-wideint-knownflags-noaddc-nosubc-nos32slct-20k
DIV_STRUCTURED_CONTROL_FLOW=1
DIV_MAX_STRUCTURED_DEPTH=5
DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 DIV_DISABLE_MULHI=1
DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 DIV_DISABLE_NEG=1
DIV_DISABLE_ABS=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1
DIV_DISABLE_SIGNED_SHR=1 DIV_DISABLE_BFIND=1
DIV_DISABLE_I32_BOUNDARY_IMMS=1 DIV_DISABLE_SET=1
DIV_DISABLE_VSUB4=1 DIV_DISABLE_S32_SLCT=1 DIV_DISABLE_ADDC=1
DIV_DISABLE_SUBC=1
DIV_MIN_BLOCKS=8 DIV_MAX_BLOCKS=48 DIV_MAX_INSTS_PER_BLOCK=18
DIV_WORKING_REGS=24 DIV_MAX_LOOP_ITERS=96 DIV_MAX_IMMEDIATE=4096
DIV_PROGRAM_BYTES=16384
seed 0x18afa3a34ce4e40a
```

The original saved fuzzer program was in
`/tmp/ptx-fuzz-structured-deep-wideint-knownflags-noaddc-nosubc-nos32slct-20k/div-1778820399-18afa3a34ce4e40a`
on the machine where this was reduced.

The initial hit happened in a run that enabled 64-bit scratch ALU generation,
but replacing the generated 64-bit operations with equivalent 32-bit operations
did not remove the divergence. This is a control-flow bug, not a 64-bit ALU
bug.

Standalone C++ bug-report repro:
`repro_ptxas_structured_loop_uniform_counter_o1.cpp`. It embeds the reduced PTX,
assembles it with `ptxas -O0`, `-O1`, `-O2`, and `-O3`, launches one 32-thread
block through the CUDA Driver API, and returns 1 when the bug is reproduced.

## Correct Scalar Trace

The reduced kernel is launched with exactly 32 threads, so `%tid.x` is `0..31`.
It does not read input.

The first counted loop executes once because `%r39` starts at 1. Inside that
loop:

```text
%p42 = (%tid.x == 32)  // false for all launched threads
%p50 = (%tid.x == 31)  // true only for tid 31
```

Both sides of the `%p50` branch run only zero-trip counted loops for the
launched threads, then all threads leave the outer `%r39` loop.

The final nested counted loop must execute once for every thread because both
loop counters start at 1:

```text
%r44 = 1
%r45 = 1
%r1 = 1 | %tid.x
```

The correct stored value for thread `t` is therefore:

```text
slot0[t] = t | 1
```

`ptxas -O0` matches that trace. `ptxas -O1`, `-O2`, and `-O3` store stale
`1` for tids `2..30`, so 29 of 32 threads are wrong.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-15 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). NVIDIA's CUDA Toolkit archive lists
CUDA Toolkit 13.2.1 as the latest release, and NVIDIA's CUDA 13.2 Update 1
release notes list CUDA nvptxcompiler version `13.2.78`.

Sources checked:

* https://developer.nvidia.com/cuda-toolkit-archive
* https://docs.nvidia.com/cuda/cuda-toolkit-release-notes/index.html

SASS below was decoded with matching CUDA 13.2.1 `nvdisasm` V13.2.78, build
`cuda_13.2.r13.2/compiler.37668154_0`.

## SASS Root Cause

At `-O0`, ptxas keeps the final counted-loop predicates in per-lane registers
and emits the final OR in the final loop body:

```text
ISETP.EQ.U32.AND P0, PT, R13, RZ, PT ;
@P0 BRA `(.L_x_26) ;
IADD3 R13, PT, PT, R13, -0x1, RZ ;
ISETP.EQ.U32.AND P0, PT, R14, RZ, PT ;
@P0 BRA `(.L_x_27) ;
IADD3 R14, PT, PT, R14, -0x1, RZ ;
LOP3.LUT R4, R2, 0x1, RZ, 0xfc, !PT ;
BRA `(.L_x_28) ;
STG.E desc[UR4][R2.64], R4 ;
```

Here `R2` is `%tid.x`, and the `LOP3.LUT` is the SASS form of
`or.b32 %r1, 1, %tid.x`.

At `-O1` and above, ptxas promotes the final loop counters `%r44` and `%r45`
to uniform registers `UR4` and `UR5`:

```text
UMOV UR4, 0x1 ;
UMOV UR5, 0x1 ;
...
.L_x_0:
UISETP.NE.U32.AND UP0, UPT, UR4, URZ, UPT ;
BRA.U !UP0, `(.L_x_17) ;
UIADD3 UR4, UPT, UPT, UR4, -0x1, URZ ;
.L_x_18:
UISETP.NE.U32.AND UP0, UPT, UR5, URZ, UPT ;
BRA.U !UP0, `(.L_x_0) ;
LOP3.LUT R5, R7, 0x1, RZ, 0xfc, !PT ;
UIADD3 UR5, UPT, UPT, UR5, -0x1, URZ ;
BRA `(.L_x_18) ;
.L_x_17:
STG.E desc[UR6][R2.64], R5 ;
```

The predecessor control flow is divergent because `%p50 = (%tid.x == 31)`.
Tid 31 reaches the final nested loop as a separate subgroup and decrements
`UR4`/`UR5` to zero. When tids `0..30` arrive, the uniform counters are already
zero, so those lanes skip the final `LOP3.LUT` and store the stale value `1`.

This is the same broad class as m001 (uniform-register analysis on loop
predicates), but it is a different minimized control-flow shape: the bad
uniform values are the final nested-loop trip counters after divergent
structured control flow. The fuzzer now has `DIV_DISABLE_STRUCTURED_LOOPS=1` so
we can keep fuzzing structured if/else shapes without repeatedly rediscovering
this counter-uniformization bug.
