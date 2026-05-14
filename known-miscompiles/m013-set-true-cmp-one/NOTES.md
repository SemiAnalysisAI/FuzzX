# m013-set-true-cmp-one

Found after adding `set.{cmp}.u32.{u32,s32}` generation while fuzzing structured
control flow with earlier known triggers disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 \
DIV_DISABLE_NEG=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1 \
DIV_DISABLE_SIGNED_SHR=1 DIV_DISABLE_BFIND=1 \
DIV_DISABLE_I32_BOUNDARY_IMMS=1 \
DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 \
DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af90f94cdf4617
```

The original saved fuzzer program was in
`/tmp/ptx-fuzz-structured-expanded-nolop3-nominmax-nomulhi-noprmt-nonot-noneg-nosignedcmp-nofunnel-nosignedshr-nobfind-noi32boundary-dp2a-set-mad24-cvt-divrem-sad-slct-dp4a/div-1778799820-18af90f94cdf4617`
on the machine where this was reduced.

The original program contained several `set.*` instructions and no `dp2a`.
Manual reduction showed this is a `set.eq.u32.u32` true-value materialization
bug.

## Correct scalar trace

PTX `set.eq.u32.u32` materializes false as `0` and true as `0xffffffff`. The
reduced PTX in `reduced.ptx` starts a two-trip counted loop with:

```text
r0 = 24
r1 = 32
r2 = 0
r3 = 2
```

Iteration 1:

```text
r3 = r3 - 1 = 1
r4 = set.eq.u32.u32(r1, r2) = set.eq(32, 0) = 0
p1 = (r4 != 1) = true
then block: r1 = 0, r2 = 0
```

Iteration 2:

```text
r3 = r3 - 1 = 0
r4 = set.eq.u32.u32(r1, r2) = set.eq(0, 0) = 0xffffffff
p1 = (r4 != 1) = true
then block: r1 = 0, r2 = 0
```

The loop exits with `r2 = 0`, so the correct stored value is `0x00000000`.
`ptxas -O0` matches that trace. `ptxas -O1`, `-O2`, and `-O3` store
`0x00000018`, which is the else-path value, as if the optimized loop treated
the true result of `set.eq.u32.u32` as `1` instead of `0xffffffff`.

Standalone C++ bug-report repro: `repro_ptxas_set_true_cmp_one_o1.cpp`. It
embeds the reduced PTX, compiles it with `ptxas -O0`, `ptxas -O1`, and
`ptxas -O2`, launches one thread through the CUDA Driver API, and returns 1
when the bug is reproduced.

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

At `-O0`, ptxas keeps the loop and lowers `set.eq.u32.u32` to a predicate plus
an integer select that writes `0xffffffff` for true:

```text
ISETP.EQ.U32.AND P0, PT, R2, R5, PT ;
SEL             R3, RZ, 0xffffffff, !P0 ;
ISETP.NE.U32.AND P0, PT, R3, 0x1, PT ;
```

At `-O2`, ptxas optimizes the loop into predicated code. The key branch
predicate is computed directly from `R0 != R5`, equivalent to using the
boolean comparison predicate instead of the materialized `set` value and then
testing that value against `1`:

```text
ISETP.NE.U32.AND P0, PT, R0, R5, PT ;
@!P0 IMAD.MOV.U32 R5, RZ, RZ, 0x18 ;
@P0  MOV          R0, RZ ;
@P0  IMAD.MOV.U32 R5, RZ, RZ, RZ ;
```

On the second loop iteration `R0 == R5`, so this optimized predicate takes the
wrong else path and stores `0x18`. The source requires comparing the
materialized `set.eq` value `0xffffffff` against `1`, which should take the
then path and store `0`.

This is distinct from the earlier signed/unsigned if-conversion bugs because
the reduced PTX uses only unsigned `set.eq.u32.u32` and unsigned `setp.ne.u32`.
