# m020-mixed-minmax-signedness-fold

Found while re-enabling `min`/`max` generation in the structured if/else-only
fuzzer after disabling known bug classes:

```text
DIV_STRUCTURED_CONTROL_FLOW=1
DIV_DISABLE_STRUCTURED_LOOPS=1
DIV_MAX_STRUCTURED_DEPTH=7
seed 0x18afa8fc39965016
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-structured-ifonly-minmax-knownflags-20k/div-1778826285-18afa8fc39965016
```

The minimized PTX in `reduced.ptx` no longer has input memory or control flow.
It is a straight-line mixed signed/unsigned `min`/`max` fold.

## Correct scalar trace

The standalone launch uses one thread, passes `n = 32`, and stores one u32.

```text
%r0 = n = 32
%r1 = %r0 << 26 = 0x80000000
%r1 = %r1 | 24 = 0x80000018
%r2 = max.u32(0, %r1) = 0x80000018
%r3 = min.s32(%r2, 0) = 0x80000018
%r4 = bfi(%r3, 24, pos=5, len=7) = 0x00000318
store %r4
```

The signed min is the important step. `0x80000018` is larger than zero as an
unsigned integer, but it is negative as an s32, so `min.s32(0x80000018, 0)` must
preserve `0x80000018`.

`ptxas -O0` stores `0x00000318`. `ptxas -O1`, `-O2`, and `-O3` store
`0x00000018`.

Standalone C++ bug-report repro:
`repro_ptxas_mixed_minmax_signedness_o2.cpp`. It embeds the reduced PTX,
compiles it with `ptxas -O0` and `ptxas -O2`, launches one thread with `n = 32`
through the CUDA Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS root cause

At `-O0`, ptxas emits the expected unsigned max followed by signed min:

```text
SHF.L.U32   R0, R0, 0x1a, RZ ;
LOP3.LUT    R0, R0, 0x18, RZ, 0xfc, !PT ;
VIMNMX.U32  R0, PT, PT, RZ, R0, !PT ;
VIMNMX      R0, PT, PT, R0, RZ, PT ;
...
STG.E       ..., R0 ;
```

At `-O2`, ptxas drops the load of `n` and the whole min/max computation. The
optimized SASS materializes `0x00000018` and stores it:

```text
HFMA2       R7, -RZ, RZ, 0, 1.430511474609375e-06 ;
STG.E       ..., R7 ;
```

This is consistent with an invalid fold of:

```text
min.s32(max.u32(0, x), 0) -> 0
```

That fold confuses the unsigned fact "`max.u32(0, x)` is unsigned-nonzero or
unsigned-at-least-zero" with the signed fact needed by the later `min.s32`.
When `x = 0x80000018`, the unsigned max returns `x`, and then the signed min
must also return `x` because it is negative.

This is related to the m003 min/max optimizer area, but it is not the same
failure mode. m003 introduced an extra candidate in a signed max chain; this
case loses the signedness distinction across a `max.u32` feeding a `min.s32`.
