# m024-prmt-cvt-u16-fold

Found while re-enabling straight-line `prmt.b32` generation with loops and
earlier known bug classes disabled:

```text
DIV_MIN_BLOCKS=1
DIV_MAX_BLOCKS=1
DIV_MIN_INSTS_PER_BLOCK=24
DIV_MAX_INSTS_PER_BLOCK=48
DIV_DISABLE_ARBITRARY_LOOPS=1
DIV_DISABLE_STRUCTURED_LOOPS=1
DIV_DISABLE_LOP3=1
DIV_DISABLE_MINMAX=1
DIV_DISABLE_SIGNED_MULHI=1
DIV_DISABLE_NOT=1
DIV_DISABLE_CNOT=1
DIV_DISABLE_ABS=1
DIV_DISABLE_SIGNED_CMP=1
DIV_DISABLE_FUNNEL=1
DIV_DISABLE_NEG=1
DIV_DISABLE_SIGNED_SHR=1
DIV_DISABLE_ADDC=1
DIV_DISABLE_SUBC=1
DIV_DISABLE_SET=1
DIV_DISABLE_S32_SLCT=1
DIV_DISABLE_VSUB4=1
seed 0x18afaf5a870b2891
```

The original saved fuzzer program was:

```text
/tmp/ptx-fuzz-straightline-prmt-known-disabled-100k/div-1778833219-18afaf5a870b2891
```

The minimized PTX in `reduced.ptx` is straight-line and has no input buffer.
It keeps an unused `in_ptr` parameter because removing that dummy first
parameter changes ptxas's optimized lowering enough to hide the bug.

## Correct scalar trace

The standalone launch uses one thread and passes `n = 32`.

```text
%r4 = 1680 * n = 0x0000d200
%r9 = 357 * 0x6000 + %r4 = 0x0086b200
%r2 = bfi.b32(%r9, 0x6000, pos=22, len=18) = 0x80006000
%r3 = prmt.b32(%r4, %r2, 0x88b9) = 0x000000ff
%r3 = cvt.s32.u16(%r3) = 0x000000ff
store %r3
```

For the `prmt` step, selector nibbles are `9, b, 8, 8`. Nibble `9` sign-fills
from byte 1 of `%r4`, which is `0xd2`, so the low output byte is `0xff`; the
other selected sign-fill bytes are zero.

`ptxas -O0` stores `0x000000ff`. `ptxas -O1`, `-O2`, and `-O3` store
`0x00000000`.

Standalone C++ bug-report repro:
`repro_ptxas_prmt_cvt_u16_o2.cpp`. It embeds the reduced PTX, compiles it with
`ptxas -O0` and `ptxas -O2`, launches one thread with `n = 32` through the CUDA
Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS root cause

At `-O0`, ptxas keeps the two-source permute and then zero-extends the low
16 bits with another `PRMT`:

```text
IMAD.U32    R0, R0, 0x690, RZ ;
...
LOP3.LUT    R3, R2, R4, R3, 0xe2, !PT ;  // bfi result
PRMT        R3, R0, 0x88b9, R3 ;          // source prmt
PRMT        R0, R3, 0x7710, RZ ;          // cvt.s32.u16
STG.E       ..., R0 ;
```

At `-O2`, ptxas collapses the chain into a single permute:

```text
IMAD        R0, R0, 0x690, RZ ;
PRMT        R5, RZ, 0xb9, R0 ;
STG.E       ..., R5 ;
```

For `n = 32`, that optimized sequence stores zero. The fold appears to drop
the nonzero high/sign-fill contribution from the second `prmt` source produced
by the preceding `bfi`, reducing the `prmt + cvt.u16` chain to a one-source
permute that is not equivalent.

This is distinct from m005's PRMT if-conversion bug: this testcase has no
branch, no predicate, no input load, and no control flow beyond the entry/ret.
