# m032-cnot-neg-ugt-fold

Found after re-enabling `sub.u32` while disabling the `0x7fffffff` /
`0x80000000` immediate generators used by m031:

```text
DIV_MIN_BLOCKS=8
DIV_MAX_BLOCKS=20
DIV_MIN_INSTS_PER_BLOCK=10
DIV_MAX_INSTS_PER_BLOCK=28
DIV_PROGRAM_BYTES=16384
DIV_WORKING_REGS=20
DIV_MAX_IMMEDIATE=65536
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
DIV_DISABLE_ADDC=1
DIV_DISABLE_SUBC=1
DIV_DISABLE_SET=1
DIV_DISABLE_S32_SLCT=1
DIV_DISABLE_NOT=1
DIV_DISABLE_I32_BOUNDARY_IMMS=1
seed 0x18afbac66a83dc30
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-acyclic-multiblock-xlarge-imm65536-knownflags-nonot-sub-noboundary-200k/div-1778845876-18afbac66a83dc30
```

The minimized PTX in `reduced.ptx` does not read the input buffer. The dummy
`in_ptr` and `in_n` parameters are kept only to match the fuzzer ABI.

## Correct Scalar Trace

For the standalone one-thread launch, `%tid.x = 0`:

```text
%r0 = %tid.x                  = 0
%r1 = cnot.b32 %r0            = 1
%r1 = neg.s32 %r1             = 0xffffffff
%p0 = setp.gt.u32 0, %r1      = (0 > 0xffffffff) unsigned = false
%r2 = selp.b32 1, 2, %p0      = 2
store %r2
```

For the fuzzer's other lanes, `%tid.x != 0`, so `cnot.b32` produces zero,
`neg.s32` keeps zero, and `0 > 0` is also false. Every lane should store `2`.

`ptxas -O0` stores `0x00000002` for every thread. `ptxas -O1`, `-O2`, and
`-O3` store `0x00000001` for `tid.x == 0`, as if the optimizer folded the
unsigned predicate to `%tid.x == 0`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS Root Cause

At `-O0`, ptxas preserves the value semantics through the `cnot` and `neg`
before doing the unsigned compare:

```text
ISETP.EQ.U32.AND    P0, PT, R2, RZ, PT ;
SEL                 R0, RZ, 0xffffffff, !P0 ;
IADD3               R0, PT, PT, RZ, -R0, RZ ;
IADD3               R0, PT, PT, RZ, -R0, RZ ;
ISETP.GT.U32.AND    P0, PT, RZ, R0, PT ;
SEL                 R0, 0x1, 0x2, P0 ;
```

For `tid.x == 0`, the value being compared is `0xffffffff`, so the unsigned
`0 > 0xffffffff` predicate is false. For `tid.x != 0`, the value is zero, so
`0 > 0` is false.

At `-O1` and above, ptxas folds the whole value/predicate chain to:

```text
ISETP.EQ.U32.AND    P0, PT, R5, RZ, PT ;
SEL                 R5, 0x1, 0x2, P0 ;
```

That makes the true arm fire exactly for `tid.x == 0`, but the PTX predicate
must be false for all lanes.
