# m027-subc-shr-mul-borrow-fold

Found while checking whether `sub.cc.u32` / `subc.u32` could be re-enabled in
the straight-line profile after keeping the earlier known triggers disabled:

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
DIV_DISABLE_SET=1
DIV_DISABLE_S32_SLCT=1
seed 0x18afb614adabf64d
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-straightline-addc-subc-enabled-noset-nosignedcmp-noabs-shl-disabled-100k/div-1778840588-18afb614adabf64d
```

The minimized PTX in `reduced.ptx` has no input-buffer dependency. The dummy
`in_ptr` parameter is intentionally kept: removing it changes ptxas's optimized
lowering enough to hide the bug.

## Correct scalar trace

The standalone launch uses one thread and passes `n = 32`.

```text
%r1 = n >> 9              = 0
%r1 = %r1 * 3             = 0
%r0 = 0xf0000000
%r2 = sub.cc.u32 %r0, %r1 = 0xf0000000, borrow-out 0
%r3 = subc.u32 %r0, 31    = 0xf0000000 - 31 - 0
                            0xefffffe1
store %r3
```

`ptxas -O0` stores `0xefffffe1`. `ptxas -O1`, `-O2`, and `-O3` store
`0xefffffe0`, as if `subc.u32` consumed an incorrect borrow-in of 1.

Standalone C++ bug-report repro:
`repro_ptxas_subc_shr_mul_borrow_o2.cpp`. It embeds the reduced PTX, compiles
it with `ptxas -O0` and `ptxas -O2`, launches one thread with `n = 32` through
the CUDA Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS root cause

At `-O0`, ptxas keeps the source expression and computes the borrow from the
actual `0xf0000000 - 0` subtract:

```text
SHF.R.U32.HI R0, RZ, 0x9, R0 ;
IMAD.U32     R0, R0, 0x3, RZ ;
MOV          R2, 0xf0000000 ;
IADD3        RZ, P0, PT, R2, -R0, RZ ;
LOP3.LUT     R0, RZ, 0x1f, RZ, 0x33, !PT ;
IADD3.X      R0, PT, PT, R2, R0, RZ, P0, !PT ;
STG.E        ..., R0 ;
```

For `n = 32`, the shifted-and-multiplied source is zero, so the `IADD3`
corresponding to `sub.cc.u32` must not report a borrow. The final `IADD3.X`
therefore stores `0xefffffe1`.

At `-O1` and above, ptxas still computes the shifted-and-multiplied value, but
the carry/borrow predicate is derived from a different expression:

```text
SHF.R.U32.HI R0, RZ, 0x9, R0 ;
IMAD         R0, R0, 0x3, RZ ;
IMAD.MOV     R5, RZ, RZ, -R0 ;
IADD3        RZ, P0, PT, R5, -R0, RZ ;
IMAD.MOV.U32 R5, RZ, RZ, -0x10000000 ;
IADD3.X      R5, PT, PT, R5, -0x20, RZ, P0, !PT ;
STG.E        ..., R5 ;
```

The optimized SASS uses the predicate from `(-x) - x`, where
`x = (n >> 9) * 3`, instead of the predicate from `0xf0000000 - x`. For
`x = 0`, the source subtract has no borrow, but the optimized cubin feeds a
borrow into `subc` and stores one less than the PTX semantics require. This is
related to m018's `subc` borrow-folding class, but it does not use `cnot` or
`shl`, so suppressing `shl` is not enough; the fuzzer needs
`DIV_DISABLE_SUBC=1` to avoid this known root.
