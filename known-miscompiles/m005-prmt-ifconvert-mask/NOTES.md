# m005-prmt-ifconvert-mask

Found by expanding structured-control-flow fuzzing after disabling explicit
`lop3.b32`, `min/max`, and `mul.hi` generation:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 \
DIV_MAX_INSTS_PER_BLOCK=10 DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 \
DIV_MAX_IMMEDIATE=1024
seed 0x18af806c82363626
```

The original saved fuzzer program is in
`/tmp/fuzzx-structured-expanded-nolop3-nominmax-nomulhi/div-1778781666-18af806c82363626`
on the machine where this was reduced. The minimized PTX in `reduced.ptx` no
longer reads input memory and launches a single thread.

## Correct scalar trace

The standalone launch passes `x = 0xdeaa8397` and `n = 32`. Since `n != 0`,
the branch goes to `then`:

```text
%r2 = prmt.b32(x, n, 0x9)
%r2 = %r2 & 255
```

For generic `prmt.b32`, selector nibble `0x9` selects byte 1 of the first
source operand and emits its sign byte. Byte 1 of `x` is `0x83`, whose high bit
is set, so the selected sign byte is `0xff`. The final `and 255` keeps only
that low byte.

The correct output is therefore `0x000000ff`. `ptxas -O0` stores
`0x000000ff`. `ptxas -O2` and `-O3` store `0x00000000`.

Standalone C++ bug-report repro: `repro_ptxas_prmt_ifconvert_o2.cpp`. It
embeds the reduced PTX, compiles it with `ptxas -O0` and `ptxas -O2`, launches
one thread with `x = 0xdeaa8397` and `n = 32` through the CUDA Driver API, and
returns 1 when the bug is reproduced.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-14 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). SASS below was decoded with matching
CUDA 13.2.1 `nvdisasm` V13.2.78, build `cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

Without the `and`, `ptxas -O2` keeps the first PRMT source:

```text
@P0 LDC  R7, c[0x0][0x388] ;       // x
@P0 PRMT R5, R7, 0x9, R0 ;         // source is x
```

With `and 255`, `ptxas -O2` if-converts the branch and folds the mask into the
PRMT, but drops `x` entirely:

```text
LDC     R0, c[0x0][0x38c] ;        // n
ISETP.NE.U32.AND P0, PT, R0, RZ, PT ;
@!P0    IMAD.MOV.U32 R5, RZ, RZ, RZ ;
@P0     PRMT R5, RZ, 0x9, R0 ;     // wrong: first source is zero
STG.E   desc[UR4][R2.64], R5 ;
```

The source PTX is `prmt.b32 %r2, %r0, %r1, 0x9`, where `%r0` is `x`. Selector
`0x9` reads byte 1 of that first source. Replacing the first source with `RZ`
makes the selected sign byte zero, which is not equivalent for inputs like
`x = 0xdeaa8397`.

Directly predicated PTX:

```text
@%p0 prmt.b32 ...
@%p0 and.b32 ...
```

does not reproduce. The bad transformation appears to be in the branch
if-conversion plus PRMT-mask folding path. This is not m001's loop predicate
bug, m002's explicit `lop3.b32` bug, m003's signed-max chain bug, or m004's
`mul.hi` loop trip-count bug.
