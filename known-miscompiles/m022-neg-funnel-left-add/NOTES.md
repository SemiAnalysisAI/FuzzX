# m022-neg-funnel-left-add

Found while continuing acyclic arbitrary-CFG fuzzing after disabling the known
`cnot`, `slct`, and `set` bug classes:

```text
DIV_DISABLE_ARBITRARY_LOOPS=1
DIV_DISABLE_STRUCTURED_LOOPS=1
DIV_DISABLE_LOP3=1
DIV_DISABLE_MINMAX=1
DIV_DISABLE_MULHI=1
DIV_DISABLE_PRMT=1
DIV_DISABLE_NOT=1
DIV_DISABLE_CNOT=1
DIV_DISABLE_NEG=1
DIV_DISABLE_ABS=1
DIV_DISABLE_S32_SLCT=1
DIV_DISABLE_SET=1
seed 0x18afad3eb41c4c9c
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-arbitrary-acyclic-funnel-nocnot-noslct-noset-knownflags-100k/div-1778830870-18afad3eb41c4c9c
```

The minimized PTX in `reduced.ptx` is straight-line and has no input buffer.
It launches one thread, receives `n = 32`, and stores one u32.

## Correct scalar trace

For `shf.l.wrap.b32 d, a, b, c`, the relevant case here is:

```text
d = (a >> (32 - c)) | (b << c)
```

The standalone launch uses `n = 32` and `c = 28`:

```text
%r0 = n = 32
%r0 = 0 - %r0 = 0xffffffe0
%r0 = shf.l.wrap.b32(%r0, 0, 28)
    = (0xffffffe0 >> 4) | (0 << 28)
    = 0x0ffffffe
%r0 = %r0 + 1 = 0x0fffffff
store %r0
```

So the correct output is `0x0fffffff`. `ptxas -O0` stores this value.
`ptxas -O1`, `-O2`, and `-O3` store `0xffffffff`.

Standalone C++ bug-report repro:
`repro_ptxas_neg_funnel_left_add_o2.cpp`. It embeds the reduced PTX, compiles
it with `ptxas -O0` and `ptxas -O2`, launches one thread with `n = 32` through
the CUDA Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS root cause

At `-O0`, ptxas negates `n`, emits the left funnel shift, then adds one:

```text
IADD3           R0, PT, PT, RZ, -R0, RZ ;
SHF.L.W.U32.HI  R0, R0, 0x1c, RZ ;
IADD3           R0, PT, PT, R0, 0x1, RZ ;
STG.E           ..., R0 ;
```

At `-O1` and above, ptxas folds the funnel shift and add into a single
`LEA.HI`:

```text
LDC             R5, c[0x0][0x388] ;
LEA.HI          R5, -R5, 0x1, RZ, 0x1c ;
STG.E           ..., R5 ;
```

For `n = 32`, the source PTX requires logical extraction of the high four bits
after negation:

```text
(0xffffffe0 >> 4) + 1 = 0x0fffffff
```

The optimized cubin stores `0xffffffff`, which is consistent with the folded
sequence sign-extending the shifted negative value instead of performing the
logical funnel-shift extraction required by PTX. This is distinct from m021's
right-funnel `cnot` case: this one needs no `cnot`, uses `shf.l.wrap.b32`, and
the wrong result is a sign-extension-shaped `0xf0000000` delta.
