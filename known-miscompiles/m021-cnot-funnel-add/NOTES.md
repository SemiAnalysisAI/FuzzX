# m021-cnot-funnel-add

Found after re-enabling `shf.{l,r}.wrap.b32` generation while fuzzing acyclic
arbitrary control flow with known bug classes disabled:

```text
DIV_DISABLE_ARBITRARY_LOOPS=1
DIV_DISABLE_STRUCTURED_LOOPS=1
DIV_DISABLE_LOP3=1
DIV_DISABLE_MINMAX=1
DIV_DISABLE_MULHI=1
DIV_DISABLE_PRMT=1
DIV_DISABLE_NOT=1
DIV_DISABLE_NEG=1
DIV_DISABLE_ABS=1
seed 0x18afab67c8c3bac9
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-arbitrary-acyclic-funnel-knownflags-50k/div-1778828893-18afab67c8c3bac9
```

The minimized PTX in `reduced.ptx` is straight-line and has no input buffer.
It launches one thread, receives `n = 32`, and stores one u32.

## Correct scalar trace

```text
%r0 = n = 32
%r0 = cnot.b32(%r0) = 0
%r0 = shf.r.wrap.b32(%r0, 16, 19)
    = (%r0 >> 19) | (16 << (32 - 19))
    = 0x00020000
%r0 = %r0 + 0x00aad528 = 0x00acd528
store %r0
```

So the correct output is `0x00acd528`. `ptxas -O0` stores this value.
`ptxas -O1`, `-O2`, and `-O3` store `0x00a8d528`, exactly `0x00040000`
lower than the correct value.

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

At `-O0`, ptxas lowers `cnot.b32`, keeps the funnel shift, then adds the
constant:

```text
ISETP.EQ.U32.AND P0, PT, R0, RZ, PT ;
SEL              R0, RZ, 0xffffffff, !P0 ;
IADD3            R0, PT, PT, RZ, -R0, RZ ;
MOV              R3, 0x10 ;
SHF.R.W.U32      R0, R0, 0x13, R3 ;
IADD3            R0, PT, PT, R0, 0xaad528, RZ ;
STG.E            ..., R0 ;
```

At `-O1` and above, ptxas folds the funnel shift and add into a `LEA.HI`
sequence:

```text
IMAD.MOV.U32     R5, RZ, RZ, 0xaad528 ;
ISETP.EQ.U32.AND P0, PT, RZ, UR6, PT ;
SEL              R0, RZ, 0xffffffff, !P0 ;
LEA.HI           R5, -R0, R5, 0x10, 0xd ;
STG.E            ..., R5 ;
```

For `n = 32`, the `cnot` result is zero, so the PTX requires:

```text
shf.r.wrap.b32(0, 16, 19) + 0x00aad528
= 0x00020000 + 0x00aad528
= 0x00acd528
```

The optimized cubin stores:

```text
0x00aad528 - 0x00020000 = 0x00a8d528
```

This points at the optimized `shf.r.wrap + add` fold/lowering: the contribution
from the high funnel operand is subtracted instead of added. Storing the raw
`shf.r.wrap` result without the following `add.u32` does not reproduce, and
replacing the `cnot` result with a literal zero also does not reproduce. This is
therefore distinct from m008's loop-unrolled funnel-shift recurrence bug.
