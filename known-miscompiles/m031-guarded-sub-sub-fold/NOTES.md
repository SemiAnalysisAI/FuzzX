# m031-guarded-sub-sub-fold

Found after disabling the m030 `not.b32` trigger and continuing the same larger
acyclic multi-block run:

```text
DIV_MIN_BLOCKS=6
DIV_MAX_BLOCKS=16
DIV_MIN_INSTS_PER_BLOCK=8
DIV_MAX_INSTS_PER_BLOCK=24
DIV_PROGRAM_BYTES=12288
DIV_WORKING_REGS=16
DIV_MAX_IMMEDIATE=2048
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
DIV_DISABLE_NOT=1
seed 0x18afb9c24d6031b6
```

The original saved fuzzer program was:

```text
/tmp/ptx-fuzz-acyclic-multiblock-large-imm2048-knownflags-nonot-100k/div-1778844643-18afb9c24d6031b6
```

The minimized PTX in `reduced.ptx` does not read the input buffer. The dummy
`in_ptr` and `in_n` parameters are kept only to match the fuzzer ABI.

## Correct Scalar Trace

For the fuzzer's 32-thread launch, `%tid.x` is in `[0, 31]`; for the standalone
one-thread launch, `%tid.x = 0`. In both cases, `%p0 = (32 != %tid.x)` is true
and the branch to `exit` is not taken.

```text
%r1 = 1
%r2 = 0x80000000 - %r1       = 0x7fffffff
%r1 = %r1 - %r2              = 1 - 0x7fffffff
                                0x80000002
store %r1
```

`ptxas -O0` stores `0x80000002`. `ptxas -O1`, `-O2`, and `-O3` store
`0x80000000`, as if the optimizer folded `x - (0x80000000 - x)` to
`-0x80000000` and dropped the required `2*x` term.

Standalone C++ bug-report repro:
`repro_ptxas_guarded_sub_sub_o2.cpp`. It embeds the reduced PTX, compiles it
with `ptxas -O0` and `ptxas -O2`, launches one thread through the CUDA Driver
API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS Root Cause

At `-O0`, ptxas emits the two subtractions in the guarded path:

```text
MOV                 R2, 0x1 ;
ISETP.NE.U32.AND    P0, PT, R0, 0x20, PT ;
...
@P0 BRA             ...
IADD3               R2, PT, PT, -R0, -0x80000000, RZ ;
IADD3               R0, PT, PT, R0, -R2, RZ ;
STG.E               ..., R0 ;
```

At `-O1` and above, ptxas folds the path to:

```text
ISETP.NE.U32.AND    P0, PT, R7, 0x20, PT ;
@P0 MOV             R5, 0x80000000 ;
STG.E               ..., R5 ;
```

For `%tid.x != 32`, the guarded path is active, but the folded constant should
be `0x80000002`.
