# m037-bmsk-clz-bfi-fold

Found while continuing structured control-flow fuzzing after suppressing the
known `not.b32`, `bfind.u32`, `mul24.*`, and `mul.hi.*` root causes from
earlier runs. `bfi.b32` and `bmsk.clamp.b32` were still enabled:

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
DIV_DISABLE_MULHI=1
seed 0x18afd713bcf2bf80
```

The original saved fuzzer program was:

```text
/tmp/fuzzx-bg-after-m036-20260515T202737Z/div-1778877311-18afd713bcf2bf80
```

The minimized PTX in `reduced.ptx` does not read the input buffer, `in_n`, or
`%tid.x`. The dummy `in_ptr` and `in_n` parameters are kept only to match the
fuzzer ABI.

## Correct Scalar Trace

```text
%r10 = bmsk.clamp.b32 17, 26 = 0xfffe0000
%r2  = clz.b32 0xfffe0000 = 0
%r4  = 0 >> 24 = 0
%r5  = bfi.b32 0, 0x00800000, 28, 26 = 0x00800000
%r18 = mad.lo.u32 0x00800000, 0x00800000, 0xc8e783b6 = 0xc8e783b6
%r6  = 0x00080000 | 0xc8e783b6 = 0xc8ef83b6
%r2  = 0xc8ef83b6 & 0x00005b57 = 0x00000316
store %r2 to out[0][2]
```

`ptxas -O0` stores `0x00000316`. With affected ptxas versions, optimized
ptxas stores `0x00004316`.

Replacing any of `bmsk.clamp.b32`, `clz.b32`, `shr.u32`, `bfi.b32`, or
`mad.lo.u32` with the corresponding constant from the scalar trace removes the
bug. Removing the final `or.b32` does not remove the bug. This points to a
ptxas fold over the `bmsk` / `clz` / `bfi` / `mad.lo` value chain, not a bad
runtime execution of one instruction in isolation.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-15 with CUDA Toolkit 13.2 Update 1 ptxas, the
latest NVIDIA CUDA Toolkit listed on NVIDIA's CUDA Toolkit Archive on
2026-05-15:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this root cause, use `DIV_DISABLE_BMSK=1` or
`DIV_DISABLE_BFI=1`; the minimized chain requires both generated idioms.
