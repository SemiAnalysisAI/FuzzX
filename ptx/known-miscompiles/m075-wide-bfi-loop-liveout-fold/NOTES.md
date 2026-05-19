# m075-wide-bfi-loop-liveout-fold

Found while continuing the CUDA 13.2.78 sweep after suppressing PRMT, LOP3,
f32 unary, constant memory, memory cache ops, MAD carry, high multiply, global
reductions, global atomics, register-operand bitfield forms, and wide `subc`:

```text
divergences/active-20260519-ptxas-13.2.78-post-m074-nomadcarry-nocache-noconst-nof32unary/div-1779177398-18b0e49b5de1b11a
```

The reduced program feeds the final `%r0` live-out through a loop containing a
wide bitfield insert:

```ptx
block_3:
    ...
    ld.volatile.global.s8 %r0, [%rd6 + 73];
    cvt.u64.u32   %rd6, %r7;
    cvt.u64.u32   %rd7, %r4;
    bfi.b64       %rd6, %rd6, %rd7, 58, 50;
    mov.b64       {%r0, %r11}, %rd6;
    setp.eq.u32   %p4, %r9, 0;
    @%p4 bra   loop_done_9;
    sub.u32         %r9, %r9, 1;
    bra             block_1;
```

At `-O0`, most lanes store slot 0 as `0x00000004`, with a few lanes taking
other defined values such as `0x00000048`, `0x00000374`, or `0x00000057`. At
`-O3`, 17 lanes store different values, including shifted or sign-shaped words
such as `0x00000400`, `0x04000003`, `0xff400000`, and `0xfffffff0`. The
divergence disappears when `bfi.b64` generation is disabled, while simply
disabling the separate wide-bitfield-group switch does not remove `bfi.b64`.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m075-wide-bfi-loop-liveout-fold/reduced.ptx \
  known-miscompiles/m075-wide-bfi-loop-liveout-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 17/32 tids differ, 18/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_WIDE_BFI=1`.
