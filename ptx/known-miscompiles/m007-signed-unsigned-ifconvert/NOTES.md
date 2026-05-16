# m007-signed-unsigned-ifconvert

Found by continuing expanded structured-control-flow fuzzing with explicit
`lop3.b32`, `min/max`, `mul.hi`, `prmt.b32`, and `not.b32` generation
disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 \
DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 \
DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af81f3becc9cf7
```

The original saved fuzzer program is in
`/tmp/fuzzx-structured-expanded-nolop3-nominmax-nomulhi-noprmt-nonot/div-1778783480-18af81f3becc9cf7`
on the machine where this was reduced. The minimized PTX in `reduced.ptx`
still reads one input word. The standalone repro launches one thread with
`x = 0xe4ca6123` and `n = 32`.

## Correct scalar trace

`x = 0xe4ca6123` has its high bit set, so as an `s32` it is negative:

```text
setp.le.s32 p0, x, 32  => true
```

The outer branch therefore enters `outer_then`. The inner comparison is
unsigned:

```text
setp.ge.u32 p1, 32, x  => false
```

Unsigned `0xe4ca6123` is larger than 32, so the PTX must take `inner_else`:

```text
r0 = 32 | 345 = 0x00000179
r3 remains 32
```

The correct output tuple is:

```text
{ r0, tid, x, r3 } = { 0x00000179, 0, 0xe4ca6123, 0x00000020 }
```

`ptxas -O0` matches that trace. `ptxas -O2` and `-O3` store
`r0 = 0x00000020` and `r3 = x * 8 = 0x26530918`, as if the inner unsigned
comparison were true.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-14 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). SASS below was decoded with matching
CUDA 13.2.1 `nvdisasm` V13.2.78, build `cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O3`, ptxas if-converts the nested branches and drops the inner unsigned
comparison entirely:

```text
LDG.E          R3, desc[UR4][R2.64] ;      // x
ISETP.GT.AND  P0, PT, R3, UR6, PT ;        // signed x > 32
@P0  SHF.L.U32 R7, R3.reuse, 0xb, RZ ;     // outer_else: x << 11
@!P0 SHF.L.U32 R9, R3, 0x3, RZ ;           // inner_then: x * 8
STG.E          desc[UR4][R4.64], R7 ;      // r0
STG.E          desc[UR4][R4.64+0xc], R9 ;  // r3
```

The SASS uses only the negation of the outer signed test. For the chosen input,
signed `x > 32` is false, but unsigned `32 >= x` is also false. The optimized
code therefore assumes an implication that is invalid across signed and
unsigned domains:

```text
signed x <= 32  does not imply  unsigned x <= 32
```

This is not m001's loop predicate bug, m002's explicit `lop3.b32` bug, m003's
signed-max chain bug, m004's `mul.hi` loop trip-count bug, m005's PRMT
if-conversion bug, or m006's optimizer-generated LOP3 complement bug. This
case is a signed/unsigned range-analysis bug in nested branch if-conversion.
