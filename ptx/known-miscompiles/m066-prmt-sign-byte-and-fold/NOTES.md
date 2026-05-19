# m066-prmt-sign-byte-and-fold

Found while fuzzing CUDA 13.2.78 `ptxas` after adding generic/global memory
roundtrips and suppressing the known `red.global.*` loop family:

```text
divergences/active-20260519-061758-ptxas-13.2.78-post-m065-nored/div-1779171495-18b0e3089926d0e9
```

The original reduction still contained generic byte store/load roundtrips, but
the memory operations were not required. The core reduced chain is:

```ptx
prmt.b32      %r1, %r2, %r3, 0x83c9;
and.b32       %r0, %r1, 255;
add.u32       %r0, %r0, 1;
```

The low `prmt` selector is the sign-control form for source byte 1. For lanes
where that input byte has the high bit set, the low output byte should be
`0xff`, so the masked-and-added value is `0x100`. Optimized ptxas stores `1`
for those lanes, as if the sign-control byte had folded to zero before the
`and.b32`.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m066-prmt-sign-byte-and-fold/reduced.ptx \
  known-miscompiles/m066-prmt-sign-byte-and-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 17/32 tids differ, 17/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_PRMT=1` or keep the
default generator suppressor that prevents live `prmt` results from feeding
later value-flow instructions.
