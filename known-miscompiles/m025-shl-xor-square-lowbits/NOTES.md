# m025-shl-xor-square-lowbits

Found while re-enabling a previously clean straight-line instruction group
with known bug classes disabled:

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
DIV_DISABLE_PRMT=1
DIV_DISABLE_NOT=1
DIV_DISABLE_SIGNED_DIVREM=1
DIV_DISABLE_FUNNEL=1
DIV_DISABLE_SIGNED_SHR=1
DIV_DISABLE_ADDC=1
DIV_DISABLE_SUBC=1
DIV_DISABLE_SET=1
DIV_DISABLE_S32_SLCT=1
seed 0x18afb0923a7b7bb0
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-straightline-reenabled-clean-group-200k/div-1778834537-18afb0923a7b7bb0
```

The minimized PTX in `reduced.ptx` is straight-line and has no input buffer.
It keeps an unused `in_ptr` parameter because removing that dummy first
parameter changes ptxas's optimized lowering enough to hide the bug.

## Correct scalar trace

The standalone launch uses one thread and passes `n = 32`.

```text
%r4 = n << 8          = 0x00002000
%r4 = 8 ^ %r4         = 0x00002008
%r2 = %r4 << 14       = 0x08020000
%r1 = %r2 * %r2       = 0x00000000  (low 32 bits)
%r6 = %r1 & 7         = 0x00000000
store %r6
```

More generally, `%r2` is shifted left by 14 before it is squared, so `%r2 *
%r2` has at least 28 low zero bits. Masking the low three bits must therefore
produce zero. `ptxas -O0` stores `0x00000000`. `ptxas -O1`, `-O2`, and `-O3`
store `0x00000004`.

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

At `-O0`, ptxas keeps the low-bit computation:

```text
SHF.L.U32   R0, R0, 0x8, RZ ;
LOP3.LUT    R0, R0, 0x8, RZ, 0x3c, !PT ;
SHF.L.U32   R0, R0, 0xe, RZ ;
IMAD.U32    R0, R0, R0, RZ ;
LOP3.LUT    R0, R0, 0x7, RZ, 0xc0, !PT ;
STG.E       ..., R0 ;
```

At `-O1` and above, ptxas folds the whole expression to a constant value
materialized via `HFMA2`, and stores `4`:

```text
HFMA2       R5, -RZ, RZ, 0, 2.384185791015625e-07 ;
STG.E       ..., R5 ;
```

The fold appears to lose the fact that the value is shifted left by 14 before
the squaring operation. Since a square of a 14-bit-aligned value has zero in
the low three bits, the folded result must be zero, not four.
