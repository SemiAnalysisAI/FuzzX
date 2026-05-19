# m080-ldu-signed-branch-fold

Found while continuing the CUDA 13.2.78 sweep after suppressing m079's
predicated packed-add variant and scalar 16-bit min/max:

```text
divergences/active-20260519-093159-ptxas-13.2.78-post-m079-noscalar16min/div-1779183159-18b0ed9ef4c75948
```

The reduced program branches to a predicated uniform global load for lanes
whose input word is negative under a signed comparison:

```ptx
setp.ge.s32   %p1, %r7, %r6;       // 32 >= input, signed
@%p1 bra      block_2;
bra           block_3;

block_2:
    setp.le.u32   %p4, %r6, %r7;   // input <= 32, unsigned
    @!%p4 ldu.global.u32 %r2, [%rd6 + 72];
```

For lanes with high-bit-set input words, the signed branch is taken and the
unsigned predicate allows the `ldu.global.u32`. At `-O0`, those lanes store the
uniform input word at byte offset 72, `0xefeafaea`. At `-O3`, affected lanes
store their original per-lane input words instead, as if the signed branch or
the predicated uniform load had been skipped.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m080-ldu-signed-branch-fold/reduced.ptx \
  known-miscompiles/m080-ldu-signed-branch-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 15/32 tids differ, 15/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use
`DIV_DISABLE_UNIFORM_GLOBAL_LOADS=1`.
