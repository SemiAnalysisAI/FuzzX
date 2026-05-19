# m084-multi-island-o-opt-crash

Found while continuing the CUDA 13.2.78 sweep after disabling the m083 family
via `DIV_DISABLE_RICH_HELPER_CALLS=1` (and the older suppressor list):

```text
divergences/active-20260519-183648-aligned-barriers/div-1779216229-18b10b59d40ff697
```

ptxas segfaults at every optimization level **above** `-O0`. `-O0` accepts
the same input cleanly, so this is an optimizer crash, distinct from m083
(which crashed at all opt levels including `-O0` on the orphan `.param`
shape). Reproduces on both 13.2.78 and 13.0.88.

Reproduce:

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas
$PTXAS -arch=sm_103 -O3 known-miscompiles/m084-multi-island-o-opt-crash/reduced.ptx -o /tmp/_t.cubin
```

Observed:

```text
Segmentation fault (core dumped)
```

Cross-version:

* CUDA Toolkit 13.2 Update 1 ptxas: `release 13.2, V13.2.78`,
  build `cuda_13.2.r13.2/compiler.37668154_0` — crashes at `-O1/-O2/-O3`
* CUDA Toolkit 13.0 ptxas: `release 13.0, V13.0.88`,
  build `cuda_13.0.r13.0/compiler.36424714_0` — crashes at `-O3`
* Both accept the same input at `-O0`.

## Trigger

Single-line reduction shrank the original 353-line program to a 66-line
`reduced.ptx`. Each of the following instruction *families* is required —
removing any one of them makes the crash disappear, even though the rest of
the program is unchanged:

* `cvt.pack.sat.u8.s32.b32` (saturating byte-pack)
* `bar.red.popc.u32` (CTA barrier reduction)
* `shfl.sync.up.b32` (warp shuffle)
* `elect.sync` (warp elect)
* `redux.sync.max.u32` (warp reduction)
* `createpolicy.fractional.L2` plus `ld.global.L2::cache_hint.u32`
* `cvt.rna.tf32.f32` and `cvt.rn.bf16.f32` / `cvt.f32.bf16` (bf16/tf32 cvt)
* `sub.rn.f16x2` (packed half arithmetic)

So this is an optimizer pass — likely some lowering or fold pass that runs
once it sees a sufficiently rich instruction soup — that goes wrong when all
these features are present together. No further root cause was isolated.

## Suppressor

There is no single obvious suppressor: every category in the list is needed,
and disabling any one of `DIV_DISABLE_CACHE_POLICY_HELPERS=1`,
`DIV_DISABLE_CVT_PACK=1`, `DIV_DISABLE_BF16_TF32_CVT=1`,
`DIV_DISABLE_CTA_BARRIER_REDUCTIONS=1`, `DIV_DISABLE_F16_ARITH=1`, or
`DIV_DISABLE_WARP_COLLECTIVES=1` removes the trigger for this specific
program. The fuzzer found this in ~7 minutes with the suppressor list that
also covers m083 — i.e. the family is much rarer than m083 — so we did not
add a dedicated suppressor flag.
