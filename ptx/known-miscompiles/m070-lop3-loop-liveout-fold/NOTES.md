# m070-lop3-loop-liveout-fold

Found while continuing the CUDA 13.2.78 sweep after suppressing global
reductions, global atomics, register-operand bitfield forms, and wide `subc`:

```text
divergences/active-20260519-074235-ptxas-13.2.78-post-m069-noatom-nored-noregbitfield-nowidesubc/div-1779174872-18b0e49b5de1cb17
```

The reduced program loops through a `lop3.b32` update to `%r0`, reloads `%r5`
from a volatile global vector load, and exits with `%r0` as the final live-out:

```ptx
block_1:
    lop3.b32      %r0, 1, %r5, %r0, 0x3e;
    ...
    @!%p3 ld.volatile.global.v4.u32 {%r5, %r4, %r6, %r7}, [%rd6 + 96];
    bra             block_2;

block_2:
    ...
    @%p6 bra   loop_done_9;
    sub.u32         %r9, %r9, 1;
    bra             block_1;
```

At `-O0`, the final `%r0` is the iterated `lop3` result: lanes store
`0x60fa173a` or `0x60fa173e`. At `-O3`, 16 lanes store the original input-size
value `0x20` instead, as if optimized ptxas dropped the loop-carried `lop3`
update on those lanes.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m070-lop3-loop-liveout-fold/reduced.ptx \
  known-miscompiles/m070-lop3-loop-liveout-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 16/32 tids differ, 16/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_LOP3=1`.
