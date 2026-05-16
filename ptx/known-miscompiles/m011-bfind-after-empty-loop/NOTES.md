# m011-bfind-after-empty-loop

Found by continuing expanded structured-control-flow fuzzing with known
miscompile triggers disabled, including `DIV_DISABLE_SIGNED_SHR=1`:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 \
DIV_DISABLE_NEG=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1 \
DIV_DISABLE_SIGNED_SHR=1 \
DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 \
DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af8cdd53d39c23
```

The original saved fuzzer program was in
`/tmp/fuzzx-structured-expanded-nolop3-nominmax-nomulhi-noprmt-nonot-noneg-nosignedcmp-nofunnel-nosignedshr-cvt-bfind-divrem-sad-slct-dp4a-long/div-1778795757-18af8cdd53d39c23`
on the machine where this was reduced.

The minimized PTX in `reduced.ptx` has one output pointer parameter, no input
buffer, one empty one-trip counted loop, and then a four-trip loop whose body
contains `bfind.u32 0` and a move of `0x7fffffff`.

## Correct scalar trace

The first loop executes once and only decrements its counter:

```text
r2 = 1
visit pre_loop: r2 != 0
r2 = r2 - 1 = 0
visit pre_loop: r2 == 0, exit
```

The second loop executes four times. Each iteration writes the same values:

```text
r4 = bfind.u32(0) = 0xffffffff
r0 = 0x7fffffff
```

After the second loop:

```text
r5 = r4 - r0 = 0xffffffff - 0x7fffffff = 0x80000000
```

The correct stored value is therefore `0x80000000`. `ptxas -O0` and `-O1`
match that trace. `ptxas -O2` and `-O3` store `0x7ffffffe`, which is the value
of `0xffffffff + 0x7fffffff`, not the source subtraction.

Replacing `bfind.u32 0` with `mov.u32 0xffffffff` makes `-O2` match `-O0`.
Removing the preceding empty counted loop also makes `-O2` match `-O0`.
Reducing the `bfind` loop trip count to three also stops the repro. So this is
a loop simplification/folding interaction with `bfind.u32`, not merely a bad
constant fold for `bfind.u32 0`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-14 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). NVIDIA's CUDA Toolkit archive lists
CUDA Toolkit 13.2.1 as the April 2026 release, and NVIDIA's CUDA 13.2 Update 1
release notes list CUDA NVCC version `13.2.78`. SASS below was decoded with
matching CUDA 13.2.1 `nvdisasm` V13.2.78, build
`cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O0`, ptxas keeps both loops and lowers `bfind.u32 0` to `FLO.U32 R2, RZ`.
The final subtract is preserved:

```text
FLO.U32 R2, RZ ;
MOV     R4, 0x7fffffff ;
...
IADD3   R4, PT, PT, R2, -R4, RZ ;
STG.E   ..., R4 ;
```

At `-O2`, ptxas deletes the loops and emits a constant store of the wrong
value:

```text
MOV   R7, 0x7ffffffe ;
STG.E desc[UR4][R2.64], R7 ;
```

The optimized constant is consistent with adding the `bfind.u32 0` result
(`0xffffffff`) to `0x7fffffff`, instead of subtracting `0x7fffffff` from it.
This is distinct from m009's `neg.s32` loop issue: replacing the source
`bfind.u32 0` with an equivalent `mov.u32 0xffffffff` removes this repro, so
the `bfind` operation is part of the triggering pattern.
