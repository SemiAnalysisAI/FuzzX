# m009-neg-loop-after-counted-loop

Found by continuing expanded structured-control-flow fuzzing with explicit
`lop3.b32`, `min/max`, `mul.hi`, `prmt.b32`, `not.b32`, signed-compare, and
funnel-shift generation disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 \
DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1 \
DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 \
DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af8903f471fc70
```

The original saved fuzzer program is in
`/tmp/fuzzx-structured-expanded-nolop3-nominmax-nomulhi-noprmt-nonot-nosignedcmp-nofunnel-cvt-bfind-divrem-sad-slct-dp4a/div-1778791215-18af8903f471fc70`
on the machine where this was reduced. Although the run had just enabled
`dp4a.u32.u32`, the saved program did not contain `dp4a`, `sad`, `slct`,
`div/rem`, or any disabled instruction family. The triggering source operation
is `neg.s32`.

The minimized PTX in `reduced.ptx` has one output pointer parameter, no input
buffer, one empty one-trip counted loop, and then a second one-trip counted
loop containing `neg.s32`.

## Correct scalar trace

The first loop starts with `r4 = 1` and only decrements its counter:

```text
visit pre_loop: r4 != 0
r4 = r4 - 1 = 0
visit pre_loop: r4 == 0, exit
```

Then the kernel initializes:

```text
r3 = 0x7fffffff
r6 = 1
```

The second loop executes once:

```text
visit neg_loop: r6 != 0
r6 = r6 - 1 = 0
r3 = neg.s32(0x7fffffff) = 0x80000001
visit neg_loop: r6 == 0, exit
```

The correct stored value is `0x80000001`. `ptxas -O0` matches that trace.
`ptxas -O2` and `-O3` store `0x7fffffff`, as if the second loop's negation did
not execute.

Standalone C++ bug-report repro: `repro_ptxas_neg_loop_o2.cpp`. It embeds the
reduced PTX, compiles it with `ptxas -O0` and `ptxas -O2`, launches one thread
through the CUDA Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-14 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). SASS below was decoded with matching
CUDA 13.2.1 `nvdisasm` V13.2.78, build
`cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O0`, ptxas keeps both loops. The second loop contains the expected
two's-complement negation:

```text
MOV         R2, 0x7fffffff ;
MOV         R4, R2 ;
ISETP.EQ.U32.AND P0, PT, R3, RZ, PT ;
@P0 BRA     done ;
IADD3       R3, PT, PT, R3, -0x1, RZ ;
IADD3       R4, PT, PT, RZ, -R4, RZ ;
```

At `-O3`, ptxas deletes both one-trip loops but stores the pre-negation
constant:

```text
MOV   R7, 0x7fffffff ;
STG.E desc[UR4][R2.64], R7 ;
```

The optimizer has therefore dropped the side effect of the second counted
loop. A source variant that removes the preceding counted loop and keeps only
the one-trip `neg.s32` loop compiles correctly, so this is an interaction in
loop simplification for sequential counted loops.

This is not m001's original loop predicate/latch bug: there is no control-flow
choice based on `%tid.x`, no live-out phi over multiple paths, and the reduced
source has two simple single-entry counted loops. It is also not m002's
explicit `lop3.b32` bug, m003's signed-max chain bug, m004's `mul.hi`
trip-count bug, m005's PRMT if-conversion bug, m006's optimizer-generated LOP3
complement bug, m007's signed/unsigned range-analysis bug, or m008's
funnel-shift recurrence bug.
