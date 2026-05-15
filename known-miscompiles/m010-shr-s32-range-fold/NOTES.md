# m010-shr-s32-range-fold

Found by continuing expanded structured-control-flow fuzzing with known
miscompile triggers disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_DISABLE_NOT=1 \
DIV_DISABLE_NEG=1 DIV_DISABLE_SIGNED_CMP=1 DIV_DISABLE_FUNNEL=1 \
DIV_MIN_BLOCKS=4 DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 \
DIV_WORKING_REGS=12 DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af89b7e737954e
```

The original saved fuzzer program was in
`/tmp/fuzzx-structured-expanded-nolop3-nominmax-nomulhi-noprmt-nonot-noneg-nosignedcmp-nofunnel-cvt-bfind-divrem-sad-slct-dp4a/div-1778792042-18af89b7e737954e`
on the machine where this was reduced. It was manually reduced after the
automatic reducer reached undefined PTX by deleting live definitions of the
final select's operands.

The minimized PTX in `reduced.ptx` has one output pointer parameter, no input
buffer, no control flow, and one launched thread.

## Correct scalar trace

For the standalone repro's one-thread launch, `%tid.x = 0`.

```text
r0 = 0
r1 = r0 | 0xbb7dffd2 = 0xbb7dffd2
r1 = shr.s32(r1, 3) = 0xf76fbffa
r2 = 0xc0000000
p0 = setp.ge.u32(r2, r1) = (0xc0000000 >= 0xf76fbffa) = false
r3 = selp(986, 123, p0) = 123
```

The correct stored value is therefore `0x0000007b`. `ptxas -O0` matches that
trace. `ptxas -O1`, `-O2`, and `-O3` store `0x000003da` (`986`).

If the source used `shr.u32` instead of `shr.s32`, the shifted value would be
`0x176fbffa`, the unsigned compare would be true, and `986` would be correct.
That is exactly the wrong result produced for the signed-shift source.

Standalone C++ bug-report repro: `repro_ptxas_shr_s32_range_o2.cpp`. It embeds
the reduced PTX, compiles it with `ptxas -O0` and `ptxas -O2`, launches one
thread through the CUDA Driver API, and returns 1 when the bug is reproduced.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-14 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). SASS below was decoded with matching
CUDA 13.2.1 `nvdisasm` V13.2.78, build
`cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O0`, ptxas lowers the signed shift and unsigned compare directly:

```text
LOP3.LUT        R0, R0, 0xbb7dffd2, RZ, 0xfc, !PT ;
SHF.R.S32.HI    R0, RZ, 0x3, R0 ;
MOV             R6, 0xc0000000 ;
ISETP.GE.U32.AND P0, PT, R6, R0, PT ;
MOV             R0, 0x3da ;
SEL             R0, R0, 0x7b, P0 ;
STG.E           ..., R0 ;
```

For `%tid.x = 0`, this computes `P0 = false` and stores `0x7b`.

At `-O2`, ptxas folds the entire computation to a constant store of
`0x3da`; the signed shift, compare, and select are gone:

```text
HFMA2 R5, -RZ, RZ, 0, 5.877017974853515625e-05 ;
STG.E desc[UR4][R2.64], R5 ;
```

The optimized result is consistent with treating the `shr.s32` as if it were
`shr.u32` for range folding before the unsigned compare. This is not m007's
signed/unsigned if-conversion bug: this reduced testcase has no branch and no
signed compare, only a signed right shift feeding an unsigned compare that is
constant-folded incorrectly.
