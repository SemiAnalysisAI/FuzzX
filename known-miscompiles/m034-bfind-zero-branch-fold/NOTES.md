# m034-bfind-zero-branch-fold

Found while fuzzing structured control flow after suppressing the `not.b32`
root cause from m033:

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
DIV_DISABLE_CLZ=1
DIV_DISABLE_NEG=1
DIV_DISABLE_NOT=1
DIV_DISABLE_I32_BOUNDARY_IMMS=1
seed 0x18afbf497ee5cc66
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-structured-if-xlarge-imm65536-knownflags-nonot-cnot-noneg-sub-noboundary-200k/div-1778850791-18afbf497ee5cc66
```

The minimized PTX in `reduced.ptx` does not read the input buffer or `in_n`.
The dummy `in_ptr` and `in_n` parameters are kept only to match the fuzzer ABI.

## Correct Scalar Trace

For `%tid.x == 0`, the branch is not taken:

```text
%r4 = 4
%p0 = setp.ne.u32 %tid.x, 0 = false
%r16 = bfind.u32 0 = 0xffffffff
%r4 = xor.b32 %r16, %tid.x = 0xffffffff ^ 0 = 0xffffffff
%r3 = sub.u32 %r4, 32 = 0xffffffff - 32 = 0xffffffdf
store %r3 to out[0][3]
```

For `%tid.x != 0`, the branch skips the `bfind.u32`, leaving `%r4 = 4`, so
those lanes store `4 - 32 = 0xffffffe4`.

`ptxas -O0` stores `0xffffffdf` for lane 0. With affected ptxas versions,
`ptxas -O1`, `-O2`, and `-O3` store `0xffffffe0`, as if `bfind.u32 0` had
folded to `0` instead of `0xffffffff` on the `%tid.x == 0` path.

Standalone C++ bug-report repro:
`repro_ptxas_bfind_zero_branch_o2.cpp`. It embeds the reduced PTX, compiles it
with `ptxas -O0` and `ptxas -O2`, launches through the CUDA Driver API, and
returns 1 when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS Root Cause

At `-O0`, ptxas lowers `bfind.u32 0` to `FLO.U32 RZ`, whose result is the PTX
zero-input sentinel `0xffffffff`, then xors with `%tid.x`:

```text
ISETP.NE.U32.AND    P0, PT, R0, RZ, PT ;
@P0 BRA             ...
FLO.U32             R2, RZ ;
LOP3.LUT            R2, R2, R0, RZ, 0x3c, !PT ;
IADD3               R4, PT, PT, R2, -0x20, RZ ;
STG.E               ..., R4 ;
```

At `-O1` and above, ptxas folds the whole branch merge to:

```text
IMAD.MOV.U32        R0, RZ, RZ, 0x4 ;
ISETP.NE.U32.AND    P0, PT, R5, RZ, PT ;
@!P0 MOV            R0, RZ ;
IADD3               R5, PT, PT, R0, -0x20, RZ ;
STG.E               ..., R5 ;
```

For `%tid.x == 0`, the optimized value is `0 - 32 = 0xffffffe0`. The PTX
requires `(bfind.u32 0 ^ 0) - 32 = 0xffffffdf`.
