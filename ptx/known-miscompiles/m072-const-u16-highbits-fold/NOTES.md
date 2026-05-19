# m072-const-u16-highbits-fold

Found while continuing the CUDA 13.2.78 sweep after suppressing PRMT, LOP3,
f32 unary, high multiply, global reductions, global atomics, register-operand
bitfield forms, and wide `subc`:

```text
divergences/active-20260519-ptxas-13.2.78-post-m071-nof32unary-noprmt-nolop3-nomadhi-noatom-nored/div-1779176152-18b0e49b5de1b278
```

The reduced program first puts a 64-bit volatile global load into `%r4`, then
loads a 16-bit unsigned constant into the same register and feeds it to
`add.s16x2`:

```ptx
    ld.volatile.global.b64 %rd7, [%rd6 + 80];
    mov.b64       {%r4, %r9}, %rd7;
    mov.u64       %rd6, fuzzx_const;
    ld.const.u16  %r4, [%rd6 + 38];
    ...
    add.s16x2     %r3, %r4, %r6;
```

The `ld.const.u16` should define `%r4` as the zero-extended constant
`0x00009a89`, so the high 16-bit lane of the packed add should remain zero.
At `-O0`, lanes store slot 3 as `0x00009a89 + tid.x`. At `-O3`, every lane
stores the same low half but with stale high bits, `0xbcab9a89 + tid.x`, as if
optimized ptxas preserved the prior `%r4` high half across the unsigned
constant load.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m072-const-u16-highbits-fold/reduced.ptx \
  known-miscompiles/m072-const-u16-highbits-fold/input.bin
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

For continued fuzzing past this family, use `DIV_DISABLE_CONST_MEMORY=1`.
