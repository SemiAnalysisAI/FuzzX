# m030-not-clz-predicate-fold

Found during a larger acyclic multi-block run after re-enabling ordinary
bitwise operations while keeping the earlier known triggers disabled:

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
seed 0x18afb858dc80bf24
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-acyclic-multiblock-large-imm2048-knownflags-100k/div-1778843097-18afb858dc80bf24
```

The minimized PTX in `reduced.ptx` does not read the input buffer. The dummy
`in_ptr` and `in_n` parameters are kept only to match the fuzzer ABI.

## Correct scalar trace

The standalone launch uses one thread, so `%tid.x = 0`.

```text
%r0 = %tid.x                  = 0
%r1 = mul.hi.u32(2, %r0)      = 0
%r2 = not.b32 %r1             = 0xffffffff
%p0 = (%r1 != 0)              = false
branch to exit is not taken
%r3 = clz.b32 %r2             = clz(0xffffffff) = 0
store %r3
```

For the fuzzer's 32-thread launch, `%tid.x` is in `[0, 31]`, so
`mul.hi.u32(2, %tid.x)` is still zero for every thread and the same trace
applies.

`ptxas -O0` stores `0x00000000`. `ptxas -O1`, `-O2`, and `-O3` store
`0x00000020`, as if the false path computed `clz(0)` instead of
`clz(~0)`.

Standalone C++ bug-report repro:
`repro_ptxas_not_clz_predicate_o2.cpp`. It embeds the reduced PTX, compiles it
with `ptxas -O0` and `ptxas -O2`, launches one thread through the CUDA Driver
API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS root cause

At `-O0`, ptxas preserves the `not` feeding `clz`:

```text
IMAD.HI.U32          R4, R0, 0x2, RZ ;
LOP3.LUT            R2, RZ, R4, RZ, 0x33, !PT ;  // not.b32
ISETP.NE.U32.AND    P0, PT, R4, RZ, PT ;
@P0 BRA             ...
FLO.U32             R2, R2 ;
IADD3               R2, PT, PT, -R2, 0x1f, RZ ;  // clz
STG.E               ..., R2 ;
```

On the not-taken path, `R4` is zero, `R2` is `~0`, and `clz(~0)` is zero.

At `-O1` and above, ptxas folds the guarded false path to:

```text
IMAD.HI.U32          R0, R7, 0x2, RZ ;
ISETP.NE.U32.AND    P0, PT, R0, RZ, PT ;
@!P0 MOV            R5, 0x20 ;
STG.E               ..., R5 ;
```

The `0x20` is the value of `clz(0)`, not `clz(~0)`. In other words, the
optimizer correctly infers that `%r1 == 0` on the fallthrough path, but then
appears to drop or misapply the intervening `not.b32` before folding `clz`.
