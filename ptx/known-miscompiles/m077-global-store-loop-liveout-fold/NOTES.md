# m077-global-store-loop-liveout-fold

Found while continuing the CUDA 13.2.78 sweep after suppressing PRMT, LOP3,
f32 unary, constant memory, memory cache ops, MAD carry, high multiply, global
reductions, global atomics, register-operand bitfield forms, wide `subc`, wide
`bfi`, and predicated `mad.lo`:

```text
divergences/active-20260519-ptxas-13.2.78-post-m076-nopredmad-nowidebfi/div-1779178415-18b0e49b5de1c2ad
```

The original generated program reduced to a generic-address `st.u64`/`ld.u64`
roundtrip in the output buffer. Manual substitutions showed that f32/f64 side
computations, shared memory, the generic load, and the final identity
`mad.lo.u32` were not required. The archived repro uses a non-aliasing
per-thread global store to `out[tid] + 8`, then later stores the checked
live-out to `out[tid] + 0`:

```ptx
block_1:
    mov.u32       %r7, 1;
    setp.lt.u32   %p6, %r1, %r3;
    @%p6 bra   block_4;
    bra             block_2;

block_2:
    cvt.u16.u32   %h0, %r6;
    cvt.s32.s16   %r6, %h0;
    setp.le.u32   %p10, 128, %r1;
    cvta.to.global.u64 %rd6, %rd0;
    @%p10 ld.global.u8 %r0, [%rd6 + 15];
    mad.lo.s32    %r7, %r6, 2, %r2;
    ...
    bra             block_0;

block_3:
    cvta.to.global.u64 %rd8, %rd1;
    mul.wide.u32  %rd9, %r8, 16;
    add.s64       %rd8, %rd8, %rd9;
    st.global.u32 [%rd8 + 8], %r0;
    bra             block_4;

block_4:
    mov.u32       %r1, 0;
    ...
    bra             block_1;

block_5:
    mad.lo.s32    %r5, %r0, %r7, %r6;
    mov.u32       %r0, %r5;
```

At `-O0`, the later pass through `block_1` resets `%r7` to `1`, so the final
value is `sign_extend(input_low16) + 0x32` for lanes that loaded byte 15 from
the input buffer. At `-O3`, 19 lanes use the stale earlier `%r7` value from
`block_2`, equivalent to `input + 2 * sign_extend(input_low16)`, and the final
multiply-add produces input-shaped values such as `0x28572c66`, `0x0ef7f885`,
and `0x351a1808`.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m077-global-store-loop-liveout-fold/reduced.ptx \
  known-miscompiles/m077-global-store-loop-liveout-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 19/32 tids differ, 19/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

The original seed is suppressed by disabling generic-address memory generation,
but the simplified repro shows the same stale-liveout bug with ordinary global
stores. For continued fuzzing past this family, use both
`DIV_DISABLE_GENERIC_MEMORY=1` and `DIV_DISABLE_GLOBAL_STORE_ROUNDTRIPS=1`.
