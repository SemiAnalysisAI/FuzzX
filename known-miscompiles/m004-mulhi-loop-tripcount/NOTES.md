# m004-mulhi-loop-tripcount

Found by continuing structured-control-flow fuzzing with explicit `lop3.b32`
generation disabled and `min/max` generation disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1
seed 0x18af7d6ff86f2390
```

The original saved fuzzer program is in
`/tmp/fuzzx-structured-nolop3-nominmax/div-1778778548-18af7d6ff86f2390`
on the machine where this was reduced. The minimized PTX in `reduced.ptx` no
longer reads input memory and launches a single thread.

## Correct scalar trace

The standalone launch passes `n = 32` and launches one thread, so `tid.x = 0`.
PTX scalar semantics:

```text
%r1 = tid.x = 0
%r2 = 4

loop while %r2 != 0:
  %r2 = %r2 - 1
  %r3 = 0xc4787a77
  %r3 = mul.hi.s32(%r3, 32)
  %r1 = %r1 + %r3
```

As a signed 32-bit integer, `0xc4787a77` is `-998737289`.

```text
mul.hi.s32(0xc4787a77, 32)
  = high_32_bits((-998737289) * 32)
  = high_32_bits(-31959593248)
  = -8
  = 0xfffffff8
```

The loop counter starts at 4 and is decremented once per iteration, so the loop
body executes exactly four times. The correct output is therefore:

```text
0 + 4 * 0xfffffff8 = 0xffffffe0
```

`ptxas -O0` stores `0xffffffe0`. `ptxas -O2` and `-O3` store `0xfffffff0`,
which is two high-multiply contributions instead of four.

Standalone C++ bug-report repro: `repro_ptxas_mulhi_loop_o2.cpp`. It embeds
the reduced PTX, compiles it with `ptxas -O0` and `ptxas -O2`, launches one
thread with `n = 32` through the CUDA Driver API, and returns 1 when the bug
is reproduced.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-14 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). SASS below was decoded with matching
CUDA 13.2.1 `nvdisasm` V13.2.78, build `cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O0`, ptxas emits a real counted loop. The relevant loop body is:

```text
ISETP.EQ.U32.AND P0, PT, R3, RZ, PT ;
@P0 BRA done ;
IADD3   R3, PT, PT, R3, -0x1, RZ ;
MOV     R2, 0xc4787a77 ;
IMAD.HI R2, R2, R0, RZ ;
IADD3   R2, PT, PT, R5, R2, RZ ;
MOV     R5, R2 ;
BRA     loop ;
```

At `-O2`, ptxas removes the loop, but the optimized cubin contains only two
`IMAD.HI` high-multiply contributions before the store:

```text
S2R     R5, SR_TID.X ;
LDC     R7, c[0x0][0x388] ;              // n
IMAD.HI R5, R7, -0x3b878589, R4 ;
IMAD.HI R5, R7, -0x3b878589, R4 ;
STG.E   desc[UR4][R2.64], R5 ;
```

For `n = 32`, each high-multiply contribution is `-8`. The source loop has
four iterations, so the optimized loop removal has dropped two recurrence
updates. This is not the m001 divergent loop-predicate bug, not the m002
`lop3.b32` fold bug, and not the m003 signed-max chain bug.
