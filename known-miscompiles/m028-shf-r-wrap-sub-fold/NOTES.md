# m028-shf-r-wrap-sub-fold

Found while checking whether funnel shifts could be re-enabled in the
straight-line profile with the earlier `cnot`/`neg` funnel roots suppressed:

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
DIV_DISABLE_CNOT=1
DIV_DISABLE_NEG=1
DIV_DISABLE_ABS=1
DIV_DISABLE_SIGNED_SHR=1
DIV_DISABLE_SUBC=1
DIV_DISABLE_SET=1
DIV_DISABLE_S32_SLCT=1
seed 0x18afb671dac42d30
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-straightline-funnel-enabled-nocnot-noneg-nosubc-knownflags-100k/div-1778841038-18afb671dac42d30
```

The minimized PTX in `reduced.ptx` has no input-buffer dependency. The dummy
`in_ptr` parameter is intentionally kept: removing it changes ptxas's optimized
lowering enough to hide the bug.

## Correct scalar trace

The standalone launch uses one thread and passes `n = 32`.

```text
%r1 = 0 - n                          = 0xffffffe0
%r2 = 0
%r2 = shf.r.wrap.b32(%r1, %r2, 19)   = 0x00001fff
%r3 = %r2 - 0x55555555               = 0xaaaacaaa
store %r3
```

`ptxas -O0` stores `0xaaaacaaa`. `ptxas -O1`, `-O2`, and `-O3` store
`0xaaaaaaab`, as if the funnel-shift result were zero.

Standalone C++ bug-report repro:
`repro_ptxas_shf_r_wrap_sub_o2.cpp`. It embeds the reduced PTX, compiles it
with `ptxas -O0` and `ptxas -O2`, launches one thread with `n = 32` through the
CUDA Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS root cause

At `-O0`, ptxas keeps the source computation:

```text
IADD3       R0, PT, PT, RZ, -R0, RZ ;
MOV         R2, RZ ;
SHF.R.W.U32 R0, R0, 0x13, R2 ;
IADD3       R0, PT, PT, R0, -0x55555555, RZ ;
STG.E       ..., R0 ;
```

For `n = 32`, the `sub.u32` gives `0xffffffe0`. A right funnel shift with zero
as the high source and shift count 19 keeps the low source's upper 13 bits,
so the shift result is `0x1fff`. Subtracting `0x55555555` gives
`0xaaaacaaa`.

At `-O1` and above, ptxas folds the sequence into:

```text
LDC     R5, c[0x0][0x390] ;
LEA.HI  R5, -R5, 0xaaaaaaab, RZ, 0xd ;
STG.E   ..., R5 ;
```

For `n = 32`, that stores `0xaaaaaaab`. This is the value that would result if
the `shf.r.wrap.b32` output were folded to zero before the final subtract. This
case is distinct from the earlier cnot/neg/funnel roots: it has no `cnot`, no
`neg`, no control flow, and no input buffer.
