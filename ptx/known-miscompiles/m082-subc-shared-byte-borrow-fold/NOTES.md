# m082-subc-shared-byte-borrow-fold

Found while continuing the CUDA 13.2.78 sweep with the cnot/funnel family
suppressed:

```text
divergences/active-20260519-115504-ptxas-13.2.78-post-m081-f16-cvt-nocnot/div-1779191817-18b0f56dc585d914
```

The automatic reducer cut the original 321-line program to 292 lines, but it
also removed the `sub.cc.u32` that defined the carry/borrow input to the
surviving `subc.u32`. The checked-in `reduced.ptx` restores that original
carry-setting instruction immediately before the `subc`:

```ptx
block_6:
    sub.cc.u32    %r6, %r7, %r6;
    subc.u32      %r2, %r5, %r3;
```

The live path reaches `block_6` after a private shared-memory signed-byte
roundtrip and scalar 16-bit multiply/cvt sequence in `block_5`. With `ptxas
-O0`, the final `%r2` output is `tid - 4` modulo 32-bit arithmetic. With
`ptxas -O3`, the same output is `tid + 5`.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m082-subc-shared-byte-borrow-fold/reduced.ptx \
  known-miscompiles/m082-subc-shared-byte-borrow-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 32/32 tids differ, 32/128 u32 slots differ
```

This is likely the same optimized borrow-chain family as
[`m018-subc-cnot-shift-borrow-fold`](../m018-subc-cnot-shift-borrow-fold/NOTES.md)
and
[`m027-subc-shr-mul-borrow-fold`](../m027-subc-shr-mul-borrow-fold/NOTES.md).
The surrounding producer instructions differ, but the surviving value mismatch
is at the `sub.cc.u32` / `subc.u32` pair.

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_SUBC=1`.
