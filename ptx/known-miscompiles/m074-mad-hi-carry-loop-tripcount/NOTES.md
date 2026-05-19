# m074-mad-hi-carry-loop-tripcount

Found while continuing the CUDA 13.2.78 sweep after suppressing PRMT, LOP3,
f32 unary, constant memory, memory cache ops, high multiply, global reductions,
global atomics, register-operand bitfield forms, and wide `subc`:

```text
divergences/active-20260519-ptxas-13.2.78-post-m073-nocache-noconst-nof32unary-noprmt-nolop3-noatom-nored/div-1779177046-18b0e49b5de1641e
```

The reduced program has a loop-carried `mad.hi.cc.s32` update to `%r0`. The
first trip uses the initial `%r3 = in_n`; later trips use `%r3` reloaded from
`ld.global.s8`, which sign-extends byte `0xc9` to `-55`:

```ptx
block_0:
    mad.hi.cc.s32 %r0, 3, %r3, %r0;
    ...
    ld.global.s8  %r3, [%rd6 + 6];
    setp.eq.u32   %p5, %r9, 0;
    @%p5 bra   loop_done_9;
    sub.u32         %r9, %r9, 1;
    bra             block_0;
```

At `-O0`, `%r0` reaches `0x14` after the loop. At `-O3`, every lane stores
`0x1c`, as if optimized ptxas dropped several of the loop's signed high-multiply
updates. This is likely related to m004's high-multiply loop-tripcount bug, but
the triggering instruction is the `mad.hi.cc` / `madc` carry-chain family rather
than plain `mul.hi`.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m074-mad-hi-carry-loop-tripcount/reduced.ptx \
  known-miscompiles/m074-mad-hi-carry-loop-tripcount/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 32/32 tids differ, 32/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_MAD_CARRY=1`.
