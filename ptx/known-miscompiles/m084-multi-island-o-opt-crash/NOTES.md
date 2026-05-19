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

A second hit of this family (`div-1779216522-18b10bee34e3a8c7`) came in shortly
after adding `DIV_DISABLE_CACHE_POLICY_HELPERS=1` to the suppressor list.
That variant reduces to a 59-line program with a different soup of
ingredients — `match.sync.any.b32`/`elect.sync`, `cvt.rna.tf32.f32`,
`abs.f16x2`, `cvt.pack.sat.u8.s32.b32`, and `barrier.sync.aligned` — but
shares the same overall shape: many uniform-region warp/CTA collectives and
unrelated dataflow combined with `cvt.pack` and a bf16/tf32 conversion. So
the bug is the same optimizer pass, not a per-feature bug.

The one category common to both variants is the saturating byte-pack
`cvt.pack.sat.u8.s32.b32`; removing it makes either repro stop crashing. We
adopt `DIV_DISABLE_CVT_PACK=1` as the suppressor for this family. (That is
heavier than ideal — it disables all of `cvt.pack.sat.{s16,u16,u8,s8,u4,s4,
u2,s2}` coverage from the `d1b2e4b` commit — but it is the only single flag
that gates the trigger and recently-added.)
