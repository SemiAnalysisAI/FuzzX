# m036-mulhi-control-fold

Found while fuzzing structured control flow after suppressing the `not.b32`,
`bfind.u32`, and `mul24.*` root causes:

```text
DIV_STRUCTURED_CONTROL_FLOW=1
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
DIV_DISABLE_NEG=1
DIV_DISABLE_NOT=1
DIV_DISABLE_BFIND=1
DIV_DISABLE_MUL24=1
DIV_DISABLE_I32_BOUNDARY_IMMS=1
seed 0x18afc4db5521f9ec
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-structured-if-xlarge-imm65536-knownflags-nonotxornot-nobfind-nomul24-clz-cnot-noneg-sub-noboundary-1m-20260515T145237Z/div-1778857246-18afc4db5521f9ec
```

The reduced PTX in `reduced.ptx` reads only `input[0]`. The reproducing input
word is:

```text
input[0] = 0x55ff25dc
in_n     = 32
```

All lanes compute the same value and store to `out[0][1]`, so the repeated
stores are benign. The standalone reproducer launches one thread.

On 2026-05-15, this was further reduced from 106 to 49 lines by deleting
semantically dead arithmetic, folding constants on the taken path, and removing
untaken-path scaffolding while preserving the same `0x80000092` (`-O0`) versus
`0x80000090` (`-O2`/`-O3`) result. Several apparently dead control-flow edges
still need to remain; deleting them causes optimized ptxas to stop taking the
buggy fold.

## Correct Scalar Trace

The taken path is:

```text
structured_if_1_else -> structured_if_2_done -> structured_if_3_done
```

For `input[0] = 0x55ff25dc` and `in_n = 32`:

```text
%r5  = 0xffd2cb88
%r6  = 32 * 0xffd2cb88 + 0xffd2cb88 = 0xfa2c3c88

%p6  = setp.ge.u32 19682, 0x55ff25dc = false
%r1  = mad.lo.u32 0xffd2cb88, 0xfa2c3c88, 31152 = 0x1b1079f0
%r15 = 0x1b1079f0 >> 26 = 6
%p14 = false
%r0  = 0x40000000
%r8  = 4
%r16 = 0x40000000 ^ 33145 = 0x40008179
%r4  = mul.hi.s32 6, 0x40008179 = 1
%p18 = setp.eq.u32 1, 0 = false

%r14 = mad.lo.u32 4, 0x20000000, 0xffffffff = 0x7fffffff
%r19 = 1 - 0x7fffffff = 0x80000002
%r1  = 0x80000002 + 144 = 0x80000092
%p21 = setp.le.u32 4, 0x80000002 = true
store %r1 to out[0][1]
```

`ptxas -O0` stores `0x80000092`. With affected ptxas versions, optimized
ptxas stores `0x80000090`.

## SASS Root Cause

In the `-O3` SASS, ptxas folds the relevant path into uniform-register
arithmetic. The final result is produced by adding the high word of
`6 * 0x40008179` to a folded constant:

```text
UIMAD.WIDE UR4, UR4, 0x6, URZ ;   // high word is 1
UIADD3 UR6, UPT, UPT, UR5, -0x7fffff71, URZ ;
IMAD.U32 R5, RZ, RZ, UR6 ;
STG.E desc[UR8][R2.64+0x4], R5 ;
```

For the scalar PTX trace above, the folded constant should contribute
`0x80000091`, giving `1 + 0x80000091 = 0x80000092`. The optimized SASS uses
`-0x7fffff71`, i.e. `0x8000008f`, giving `1 + 0x8000008f = 0x80000090`.

Replacing the live `mul.hi.s32 %r4, %r2, %r16` with the correct constant `1`
removes the bug, as does replacing the controlling branches with unconditional
branches. This points to a ptxas value/control-flow fold around the `mul.hi`
result, rather than a bad execution of a single SASS multiply instruction.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

For continued fuzzing past this root cause, use `DIV_DISABLE_MULHI=1` rather
than only `DIV_DISABLE_SIGNED_MULHI=1`; changing the live `%r4` multiply to
`mul.hi.u32` still reproduces because both operands are nonnegative.
