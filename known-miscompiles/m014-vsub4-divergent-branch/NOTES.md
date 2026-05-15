# m014-vsub4-divergent-branch

Found after adding more video instruction coverage while fuzzing structured
control flow with earlier known triggers disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 \
DIV_DISABLE_NEG=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1 \
DIV_DISABLE_SIGNED_SHR=1 DIV_DISABLE_BFIND=1 \
DIV_DISABLE_I32_BOUNDARY_IMMS=1 DIV_DISABLE_SET=1 \
DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 \
DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af950590716284
```

The original saved fuzzer program was in
`/tmp/ptx-fuzz-structured-expanded-knownflags-video-avrg/div-1778804426-18af950590716284`
on the machine where this was reduced.

Manual reduction showed this is a `vsub4.u32.u32.u32` constant-folding bug in
a divergent branch. It is independent of the newly-added `vavrg2/vavrg4`
instructions; the original program contained only `vsub4` from the video
family.

## Correct scalar trace

The reduced PTX in `reduced.ptx` launches one warp. Each lane starts with:

```text
r3 = tid.x
r0 = 0
r1 = tid.x
r2 = 4
```

The branch predicate is `r2 != r1`, so all lanes except lane 4 take
`then_path` and store `0`.

Lane 4 falls through to:

```text
vsub4.u32.u32.u32 r1, 0, 4, 4
```

`vsub4` performs four independent unsigned byte subtractions. The low byte is
`0 - 4 mod 256 = 0xfc`; the other byte lanes are `0 - 0 = 0`. The correct
lane-4 output is therefore:

```text
0x000000fc
```

`ptxas -O0` matches that trace. `ptxas -O1`, `-O2`, and `-O3` store
`0x8080807c` for lane 4.

Standalone C++ bug-report repro: `repro_ptxas_vsub4_branch_o1.cpp`. It embeds
the reduced PTX, compiles it with `ptxas -O0`, `ptxas -O1`, `ptxas -O2`, and
`ptxas -O3`, launches 32 threads through the CUDA Driver API, and returns 1
when the bug is reproduced.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-15 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). NVIDIA's CUDA Toolkit archive lists
CUDA Toolkit 13.2.1 as the latest release, and NVIDIA's CUDA 13.2 Update 1
release notes list CUDA Application Compiler version `13.2.78`.

Sources checked:

* https://developer.nvidia.com/cuda-toolkit-archive
* https://docs.nvidia.com/cuda/cuda-toolkit-release-notes/index.html

SASS below was decoded with matching CUDA 13.2.1 `nvdisasm` V13.2.78, build
`cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O0`, ptxas keeps the branch and lowers `vsub4` into a byte-wise subtract
sequence. The sequence uses a `0x80808080` bias and then applies a final fixup
with `LOP3`, producing the packed byte result `0x000000fc` for lane 4:

```text
ISETP.NE.U32.AND P0, PT, R3, R0, PT ;
@P0 BRA          then_path ;
LOP3.LUT        R2, R2, 0x80808080, RZ, 0xfc, !PT ;
LOP3.LUT        R0, R0, 0x7f7f7f7f, RZ, 0xc0, !PT ;
LOP3.LUT        R6, R6, 0x80808080, RZ, 0xc0, !PT ;
IADD3           R0, PT, PT, R2, -R0, RZ ;
LOP3.LUT        R0, R0, R6, RZ, 0x3c, !PT ;
```

At `-O2`, ptxas folds the branch to a single select:

```text
ISETP.NE.U32.AND P0, PT, R5.reuse, 0x4, PT ;
SEL             R5, RZ, 0x8080807c, P0 ;
STG.E           desc[UR4][R2.64], R5 ;
```

For lane 4, `P0` is false, so this stores `0x8080807c`. That value is the
biased subtract intermediate from the `-O0` lowering, not the final
packed-byte `vsub4` result. This is distinct from the earlier `set` true-value
bug and from the scalar min/max and signed/unsigned compare bugs.

The fuzzer now has `DIV_DISABLE_VSUB4=1` / `emit_vsub4=false` so we can keep
the rest of the video instruction coverage while avoiding this known root
cause in future runs.
