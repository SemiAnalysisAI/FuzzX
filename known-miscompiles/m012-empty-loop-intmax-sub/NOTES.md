# m012-empty-loop-intmax-sub

Found after enabling `mad24` generation while fuzzing structured control flow
with earlier known triggers disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 \
DIV_DISABLE_NEG=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1 \
DIV_DISABLE_SIGNED_SHR=1 DIV_DISABLE_BFIND=1 \
DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 \
DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af8ecf76249973
```

The original saved fuzzer program was in
`/tmp/fuzzx-structured-expanded-nolop3-nominmax-nomulhi-noprmt-nonot-noneg-nosignedcmp-nofunnel-nosignedshr-nobfind-mad24-cvt-divrem-sad-slct-dp4a/div-1778797532-18af8ecf76249973`
on the machine where this was reduced.

The original program contained `cvt.u32.u8` in a dead `else` block and no
`mad24`. Manual reduction showed neither is involved.

The minimized PTX in `reduced.ptx` has an unused first pointer parameter, an
output pointer as the second parameter, one empty one-trip counted loop, and
then a four-trip loop whose body writes `0x7fffffff`.

## Correct scalar trace

The first loop executes once and only decrements its counter:

```text
r1 = 1
visit pre_loop: r1 != 0
r1 = r1 - 1 = 0
visit pre_loop: r1 == 0, exit
```

The second loop executes four times. Each iteration writes the same value:

```text
r0 = 0x7fffffff
```

After the second loop:

```text
r3 = 430 - r0 = 430 - 0x7fffffff = 0x800001af  (mod 2^32)
```

The correct stored value is therefore `0x800001af`. `ptxas -O0` matches that
trace. `ptxas -O2` stores `0x800001ad`, as if the value leaving the loop were
`0x80000001` instead of `0x7fffffff`.

Removing the empty pre-loop makes `-O2` correct. Reducing the value loop from
four trips to three also makes `-O2` correct. The original `or.b32 893,
0x7fffffff` can be replaced with `mov.u32 0x7fffffff` and the bug remains.

Standalone C++ bug-report repro: `repro_ptxas_intmax_loop_o2.cpp`. It embeds
the reduced PTX, compiles it with `ptxas -O0` and `ptxas -O2`, launches one
thread through the CUDA Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-14 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). NVIDIA's CUDA Toolkit archive lists
CUDA Toolkit 13.2.1 as the latest release, and NVIDIA's CUDA 13.2 Update 1
release notes list CUDA Application Compiler version `13.2.78`.

Sources checked:

* https://developer.nvidia.com/cuda-toolkit-archive
* https://docs.nvidia.com/cuda/cuda-toolkit-release-notes/index.html

SASS below was decoded with matching CUDA 13.2.1 `nvdisasm` V13.2.78, build
`cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O0`, ptxas keeps both loops. The value loop sets `R0` to `0x7fffffff`,
and the final subtract is preserved:

```text
MOV     R0, 0x7fffffff ;
...
IADD3   R0, PT, PT, -R0, 0x1ae, RZ ;
STG.E   ..., R0 ;
```

At `-O2`, ptxas deletes both loops and emits a constant store of the wrong
value:

```text
HFMA2 R5, -RZ, RZ, -0.0 , 2.5570392608642578125e-05 ;  // encodes 0x800001ad
STG.E desc[UR4][R2.64], R5 ;
```

The optimized constant is exactly two less than the correct value:

```text
correct: 430 - 0x7fffffff = 0x800001af
O2:      0x800001ad = 430 - 0x80000001
```

This looks like a loop simplification/folding bug around a four-trip counted
loop that writes the signed-positive boundary value `0x7fffffff`, with a
preceding empty counted loop as part of the trigger.
