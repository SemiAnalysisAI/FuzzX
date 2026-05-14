# m008-funnel-shift-loop-unroll

Found by continuing expanded structured-control-flow fuzzing with explicit
`lop3.b32`, `min/max`, `mul.hi`, `prmt.b32`, `not.b32`, and signed-compare
generation disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 \
DIV_DISABLE_SIGNED_CMP=1 \
DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 \
DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af838295a7233b
```

The original saved fuzzer program is in
`/tmp/ptx-fuzz-structured-expanded-nolop3-nominmax-nomulhi-noprmt-nonot-nosignedcmp/div-1778785180-18af838295a7233b`
on the machine where this was reduced.

The minimized PTX in `reduced.ptx` has one output pointer parameter, no input
buffer, one six-trip loop, and one `shf.r.wrap.b32` recurrence. The standalone
repro launches one thread, so `%tid.x = 0`.

## Correct scalar trace

The loop starts with:

```text
r0 = 32
r9 = tid = 0
r11 = 32
```

Each iteration executes:

```text
r8  = 0 - r0
r10 = r11 & r9
r11 = r11 * r10 + r11
r0  = r10 ^ 4096
r3  = shf.r.wrap.b32(r8, 469, 9)
r9  = r3 + 4
```

For shift amount 9:

```text
shf.r.wrap.b32(a, 469, 9) = (a >> 9) | (469 << 23)
```

The six-iteration scalar trace for tid 0 is:

```text
iter  r10        r11 after   r0 after    r3         r9 after
  0   00000000   00000020    00001000    eaffffff   eb000003
  1   00000000   00000020    00001000    eafffff8   eafffffc
  2   00000020   00000420    00001020    eafffff8   eafffffc
  3   00000420   00110820    00001420    eafffff7   eafffffb
  4   00110820   14930c20    00111820    eafffff5   eafffff9
  5   00930c20   81e61020    00931c20    eafff773   eafff777
```

The correct stored value is `0x00931c20`. `ptxas -O0` matches that trace.
`ptxas -O2` and `-O3` store `0x14131c20`.

Standalone C++ bug-report repro: `repro_ptxas_funnel_loop_o2.cpp`. It embeds
the reduced PTX, compiles it with `ptxas -O0` and `ptxas -O2`, launches one
thread through the CUDA Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-14 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). SASS below was decoded with matching
CUDA 13.2.1 `nvdisasm` V13.2.78, build
`cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O0`, ptxas keeps the source loop and emits an actual funnel shift:

```text
IADD3       R4, PT, PT, RZ, -R4, RZ ;
LOP3.LUT    R2, R0, R2, RZ, 0xc0, !PT ;   // r10 = r11 & r9
IMAD.U32    R0, R0, R2, R0 ;              // r11 = r11*r10 + r11
LOP3.LUT    R2, R2, 0x1000, RZ, 0x3c, !PT ; // r0 = r10 ^ 4096
MOV         R8, 0x1d5 ;
SHF.R.W.U32 R4, R4, 0x9, R8 ;             // r3 = shf.r.wrap(-old_r0,469,9)
IADD3       R8, PT, PT, R4, 0x4, RZ ;     // r9 = r3 + 4
```

At `-O2`, ptxas fully unrolls the loop and rewrites the loop-carried
`shf.r.wrap` values into `LEA.HI` expressions. The final stored value is
computed by:

```text
LEA.HI   R0, -R9, R4, 0x1d5, 0x17 ;
LOP3.LUT R5, R7, 0x1000, R0, 0x6c, !PT ;
STG.E    desc[UR4][R2.64], R5 ;
```

For this `LOP3.LUT` truth table, `0x6c` implements:

```text
R5 = (R7 & R0) ^ 0x1000
```

The source PTX requires the final AND input to be:

```text
0x14930c20 & 0xeafffff9 = 0x00930c20
```

and therefore:

```text
0x00930c20 ^ 0x1000 = 0x00931c20
```

The optimized cubin instead stores `0x14131c20`, so the mask/value feeding the
collapsed final expression is not the PTX `shf.r.wrap` recurrence value.

As a sanity check, replacing the source `shf.r.wrap.b32` with equivalent PTX:

```text
shr.u32 %r3, %r8, 9;
or.b32  %r3, %r3, 0xea800000;
```

makes `-O2` match `-O0`. This points at optimized funnel-shift recurrence
lowering after loop unrolling. It is not m001's loop predicate bug, m002's
explicit `lop3.b32` bug, m003's signed-max chain bug, m004's `mul.hi`
trip-count bug, m005's PRMT if-conversion bug, m006's optimizer-generated LOP3
complement bug, or m007's signed/unsigned range-analysis bug.
