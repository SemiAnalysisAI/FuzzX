# m029-addc-mul-carry-fold

Found during a longer straight-line run after re-enabling `add.cc.u32` /
`addc.u32` while keeping the earlier known triggers disabled:

```text
DIV_MIN_BLOCKS=1
DIV_MAX_BLOCKS=1
DIV_MIN_INSTS_PER_BLOCK=24
DIV_MAX_INSTS_PER_BLOCK=56
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
DIV_DISABLE_SUBC=1
DIV_DISABLE_SET=1
DIV_DISABLE_S32_SLCT=1
seed 0x18afb726353ea5c4
```

The original saved fuzzer program was:

```text
/tmp/ptx-fuzz-straightline-wide-knownflags-200k/div-1778841859-18afb726353ea5c4
```

The minimized PTX in `reduced.ptx` has no input-buffer dependency. The dummy
`in_ptr` parameter is intentionally kept: removing it changes ptxas's optimized
lowering enough to hide the bug.

## Correct scalar trace

The standalone launch uses one thread and passes `n = 32`.

```text
%r0 = n * 512                 = 0x00004000
%r1 = %r0 - 72                = 0x00003fb8
%r1 = 8388608 * %r1           = 0xdc000000  (low 32 bits)
%r2 = add.cc.u32 %r1, 31      = 0xdc00001f, carry-out 0
%r3 = addc.u32 %r0, -9        = 0x00004000 + 0xfffffff7 + 0
                                0x00003ff7
store %r3
```

`ptxas -O0` stores `0x00003ff7`. `ptxas -O1`, `-O2`, and `-O3` store
`0x00003ff8`, as if `addc.u32` consumed an incorrect carry-in of 1.

Standalone C++ bug-report repro:
`repro_ptxas_addc_mul_carry_o2.cpp`. It embeds the reduced PTX, compiles it
with `ptxas -O0` and `ptxas -O2`, launches one thread with `n = 32` through the
CUDA Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS root cause

At `-O0`, ptxas keeps the carry-producing add and threads its real carry-out
into the carry-consuming add:

```text
IMAD.SHL.U32 R0, R0, 0x200, RZ ;
IADD3        R2, PT, PT, R0, -0x48, RZ ;
IMAD.SHL.U32 R2, R2, 0x800000, RZ ;
IADD3        RZ, P0, PT, R2, 0x1f, RZ ;
IADD3.X      R0, PT, PT, R0, -0x9, RZ, P0, !PT ;
STG.E        ..., R0 ;
```

For `n = 32`, `0xdc000000 + 31` does not carry, so `P0` must be false and the
final value is `0x3ff7`.

At `-O1` and above, ptxas folds the sequence to a single `LEA.X` with the
carry predicate forced true:

```text
LEA.X R5, R5, 0xfffffff7, 0x9, PT ;
STG.E ..., R5 ;
```

For `n = 32`, that stores `0x3ff8`. This is related to m017's addc
carry-folding class, but the PTX source here contains no `shl.b32`; the
problem is exposed by `mul.lo.u32` with power-of-two constants.
