# m026-shr-abs-ult-fold

Found while fuzzing straight-line PTX with the earlier known bug classes mostly
disabled and `not` re-enabled:

```text
DIV_MIN_BLOCKS=1
DIV_MAX_BLOCKS=1
DIV_MIN_INSTS_PER_BLOCK=24
DIV_MAX_INSTS_PER_BLOCK=56
DIV_DISABLE_ARBITRARY_LOOPS=1
DIV_DISABLE_STRUCTURED_LOOPS=1
DIV_DISABLE_LOP3=1
DIV_DISABLE_MINMAX=1
DIV_DISABLE_SIGNED_MULHI=1
DIV_DISABLE_BITWISE_BINOPS=1
DIV_DISABLE_SHL=1
DIV_DISABLE_PRMT=1
DIV_DISABLE_SIGNED_DIVREM=1
DIV_DISABLE_FUNNEL=1
DIV_DISABLE_SIGNED_SHR=1
DIV_DISABLE_ADDC=1
DIV_DISABLE_SUBC=1
DIV_DISABLE_SET=1
DIV_DISABLE_S32_SLCT=1
seed 0x18afb332fba2f9d6
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-straightline-not-enabled-bitwise-shl-disabled-100k/div-1778837419-18afb332fba2f9d6
```

The minimized PTX in `reduced.ptx` is straight-line and has no input-buffer
dependency. The dummy `in_ptr` parameter is kept so the standalone reproducer
uses the same three-argument ABI as the fuzzer.

## Correct scalar trace

The standalone launch uses one thread and passes `n = 32`.

```text
%r1 = 0
%r1 = %r1 >> 0        = 0
%r2 = abs.s32(32)     = 32
%r0 = %r1 - %r2       = 0xffffffe0
%p0 = (%r1 < %r0).u32 = (0 < 0xffffffe0) = true
%r0 = %p0 ? 1 : 2     = 1
store %r0
```

`ptxas -O0` stores `0x00000001`. `ptxas -O1`, `-O2`, and `-O3` store
`0x00000002`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS root cause

At `-O0`, ptxas keeps the value computation and predicate:

```text
MOV                 R2, RZ ;
SHF.R.U32.HI        R4, RZ, RZ, R2 ;
IABS                R0, R0 ;
IADD3               R0, PT, PT, R4, -R0, RZ ;
ISETP.LT.U32.AND    P0, PT, R4, R0, PT ;
MOV                 R0, 0x1 ;
SEL                 R0, R0, 0x2, P0 ;
STG.E               ..., R0 ;
```

At `-O1` and above, ptxas folds away the shift, absolute value, subtraction,
unsigned predicate, and select, then stores the false-arm constant:

```text
HFMA2               R5, -RZ, RZ, 0, 1.1920928955078125e-07 ;
STG.E               ..., R5 ;
```

The optimized SASS no longer loads `in_n` or evaluates the predicate. Runtime
output shows the materialized value is `2`. The fold appears to incorrectly
reason that `0 < (0 - abs(n))` is false, treating the subtraction as a signed or
non-wrapping expression. In PTX `sub.u32` wraps modulo 2^32, and
`setp.lt.u32` is an unsigned comparison, so for `n = 32` the predicate is true.
