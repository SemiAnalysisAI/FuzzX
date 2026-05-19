# m079-predicated-packed-add-high-half

Found while replaying a CUDA 13.2.78 sweep after adding uniform `membar`
generation:

```text
divergences/active-20260519-ptxas-13.2.78-membar-window-18b0e49b5de5ddc2/div-1779181991-18b0e49b5de654e8
```

The saved generated program did not contain a `membar`; the shifted generation
stream exposed a current `ptxas` issue around a branch-local predicated packed
add.

The checked-in reducer keeps the trigger small: only lane 19 reaches a
predicated `add.u16x2 %r4, 32, %tid.x`, then every lane stores `16 + %r4`.
For lane 19, the correct packed add result is `0x00000033`, so the final
stored value is `0x00000043`. Optimized `ptxas` stores `0x00130043`, as if the
high 16-bit lane also received `%tid.x`.

A later post-m079 sweep found the same high-half corruption with an
unpredicated signed packed add:

```text
divergences/active-20260519-093159-ptxas-13.2.78-post-m079-noscalar16min/div-1779183189-18b0ed9ef4c7ebb0
```

The extra checked-in variant, `reduced_s16x2_unpredicated.ptx`, reaches an
unpredicated `add.s16x2` only for lane 4. The correct stored value is
`0x00000004`, but optimized `ptxas` stores `0x00040004`, again filling the high
half with the lane value.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m079-predicated-packed-add-high-half/reduced.ptx \
  known-miscompiles/m079-predicated-packed-add-high-half/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 1/32 tids differ, 1/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use
`DIV_DISABLE_PACKED_ADD=1`. The original predicated variant was avoided by the
narrower `DIV_DISABLE_PREDICATED_PACKED_ADD=1`, but the later unpredicated
signed variant shows the whole packed-add family needs suppression for current
13.2.78 sweeps.
