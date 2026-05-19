# m071-f32-neg-loop-liveout-fold

Found while continuing the CUDA 13.2.78 sweep after suppressing PRMT, LOP3,
high multiply, global reductions, global atomics, register-operand bitfield
forms, and wide `subc`:

```text
divergences/active-20260519-080655-ptxas-13.2.78-post-m070-noprmt-nolop3-nomadhi-noatom-nored/div-1779175486-18b0e49b5de168b7
```

The reduced program loops through an f32 negation and conversion to a signed
integer. The final store writes `%r2`, which should be the loop-carried
`cvt.rzi.s32.f32` result:

```ptx
block_2:
    and.b32       %r12, %r7, 1023;
    cvt.rn.f32.u32 %f0, %r12;
    neg.f32       %f1, %f0;
    cvt.rzi.s32.f32 %r2, %f1;
    bra             block_6;

block_6:
    setp.eq.u32   %p14, %r11, 0;
    @%p14 bra   loop_done_11;
    sub.u32         %r11, %r11, 1;
    bra             block_2;
```

At `-O0`, lanes 0 through 13 store the loop-computed slot-2 value, either
`0x00000000` or `0xffffffe0`. At `-O3`, those same lanes store original
input-like words such as `0x22150fe3`, `0xc04c899c`, or `0x5e840355`, as if
optimized ptxas replaced the loop live-out with the pre-loop input value.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m071-f32-neg-loop-liveout-fold/reduced.ptx \
  known-miscompiles/m071-f32-neg-loop-liveout-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 14/32 tids differ, 14/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_F32_UNARY=1`.
