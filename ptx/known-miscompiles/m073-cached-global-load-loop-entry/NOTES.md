# m073-cached-global-load-loop-entry

Found while continuing the CUDA 13.2.78 sweep after suppressing PRMT, LOP3,
f32 unary, constant memory, high multiply, global reductions, global atomics,
register-operand bitfield forms, and wide `subc`:

```text
divergences/active-20260519-ptxas-13.2.78-post-m072-noconst-nof32unary-noprmt-nolop3-noatom-nored/div-1779176497-18b0e49b5de1710c
```

The reducer removed the generated mask after the bit-width load, so
`reduced.ptx` restores that original `and.b32` to keep the full `%r1` value
defined. The reduced program has divergent entry into a small loop. Lanes 2
through 31 branch to `block_3` before their first visit to `block_2`, but the
backedge from `block_3` should still execute `block_2` and load `%r1` from the
input buffer:

```ptx
block_2:
    ...
    ld.global.cg.b16 %r1, [%rd6 + 64];
    and.b32       %r1, %r1, 65535;
    bra             block_3;

block_3:
    ...
    setp.eq.u32   %p11, %r9, 0;
    @%p11 bra   loop_done_9;
    sub.u32         %r9, %r9, 1;
    bra             block_2;
```

At `-O0`, all lanes store slot 1 as the cached global-load value
`0x00002b0c`. At `-O3`, lanes 2 through 31 store their original `%tid.x` value
instead, as if optimized ptxas exited those lanes without applying the loop
body's cached load. This is likely the same divergent loop-header-entry family
as m001, but with the visible dropped update being a cached narrow global load.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m073-cached-global-load-loop-entry/reduced.ptx \
  known-miscompiles/m073-cached-global-load-loop-entry/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 30/32 tids differ, 30/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_MEMORY_CACHE_OPS=1`.
