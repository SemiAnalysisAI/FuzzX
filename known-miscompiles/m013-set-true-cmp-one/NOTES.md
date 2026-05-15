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
`/tmp/fuzzx-structured-expanded-nolop3-nominmax-nomulhi-noprmt-nonot-noneg-nosignedcmp-nofunnel-nosignedshr-nobfind-noi32boundary-dp2a-set-mad24-cvt-divrem-sad-slct-dp4a/div-1778799820-18af90f94cdf4617`
on the machine where this was reduced.

The original program contained several `set.*` instructions and no `dp2a`.
Manual reduction showed this is a `set.eq.u32.u32` true-value materialization
bug.

## Correct scalar trace

PTX `set.eq.u32.u32` materializes false as `0` and true as `0xffffffff`. The
reduced PTX in `reduced.ptx` first stores `0` to the output pointer, then runs
a one-trip counted loop with:

```text
r0 = 1
```

Iteration 1:

```text
r0 = r0 - 1 = 0
r1 = set.eq.u32.u32(r0, 0) = 0xffffffff
p0 = (r1 != 1) = true
branch back to loop, skipping the conditional store of 1
```

Next loop header:

```text
r0 == 0, so the loop exits
```

The final output remains the initial in-kernel store, `0x00000000`. `ptxas
-O0` matches that trace. `ptxas -O1`, `-O2`, and `-O3` store `0x00000001`,
as if the optimized loop treated the true result of `set.eq.u32.u32` as `1`
instead of `0xffffffff`.

Standalone C++ bug-report repro: `repro_ptxas_set_true_cmp_one_o1.cpp`. It
embeds the reduced PTX, compiles it with `ptxas -O0`, `ptxas -O1`,
`ptxas -O2`, and `ptxas -O3`, launches one thread through the CUDA Driver API,
and returns 1 when the bug is reproduced.

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
ISETP.EQ.U32.AND P0, PT, R0, RZ, PT ;
SEL             R2, RZ, 0xffffffff, !P0 ;
ISETP.NE.U32.AND P0, PT, R2, 0x1, PT ;
@P0 BRA         loop ;
```

At `-O2`, ptxas optimizes the loop into predicated code. The key branch
predicate is computed from the loop value directly, equivalent to replacing
`setp.ne(set.eq(r0, 0), 1)` with `r0 != 0`:

```text
UIADD3           UR4, UPT, UPT, UR4, -0x1, URZ ;
ISETP.NE.U32.AND P0, PT, RZ, UR4, PT ;
@!P0 MOV         R5, 0x1 ;
@!P0 STG.E       desc[UR6][R2.64], R5 ;
```

After the decrement, `UR4 == 0`. The source requires materializing the true
`set.eq` result as `0xffffffff`, then comparing that value against `1`, so the
branch back to the loop should be taken and the store of `1` skipped. The
optimized code instead makes `P0` false and executes the `@!P0` store.

This is distinct from the earlier signed/unsigned if-conversion bugs because
the reduced PTX uses only unsigned `set.eq.u32.u32` and unsigned `setp.ne.u32`.
