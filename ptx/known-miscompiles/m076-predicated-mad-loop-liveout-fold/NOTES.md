# m076-predicated-mad-loop-liveout-fold

Found while continuing the CUDA 13.2.78 sweep after suppressing PRMT, LOP3,
f32 unary, constant memory, memory cache ops, MAD carry, high multiply, global
reductions, global atomics, register-operand bitfield forms, wide `subc`, and
wide `bfi`:

```text
divergences/active-20260519-ptxas-13.2.78-post-m075-nowidebfi-nomadcarry-nocache/div-1779177848-18b0e49b5de1c011
```

The original 111-line program reduced to an 83-line repro after removing an
irrelevant f32 side computation and restoring the byte-load mask that keeps the
loaded value defined as a 32-bit integer. The remaining failure is a
loop-carried live-out updated by a predicated `mad.lo.u32`:

```ptx
block_3:
    setp.ne.u32   %p7, 10, %r3;
    @%p7 mad.lo.u32 %r5, %r0, %r1, %r2;
    bra             block_4;

block_4:
    setp.eq.u32   %p9, %r9, 0;
    @%p9 bra   loop_done_9;
    sub.u32         %r9, %r9, 1;
    bra             block_3;
loop_done_9:
    bra             block_5;

block_5:
    setp.ge.u32   %p10, %r5, %r5;
    and.b32       %r10, %r3, 31;
    @!%p10 shl.b32  %r4, %r2, %r10;
    setp.lt.u32   %p11, %r5, %r3;
    @!%p11 mov.u32 %r1, %lanemask_gt;
```

At `-O0`, slot 1 stores `%lanemask_gt` for every lane: `0xfffffffe` for lane
0, `0xfffffffc` for lane 1, continuing down to `0x00000000` for lane 31. At
`-O3`, 16 lanes instead leave `%r1` as the original `%tid.x`, such as
`0x00000000`, `0x00000003`, `0x00000005`, and `0x0000001f`. That is consistent
with optimized ptxas dropping or misfolding the loop-body `mad.lo.u32` update
before the final `%r5 < %r3` predicate.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m076-predicated-mad-loop-liveout-fold/reduced.ptx \
  known-miscompiles/m076-predicated-mad-loop-liveout-fold/input.bin
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

For continued fuzzing past this family, use `DIV_DISABLE_PREDICATED_MAD=1`.
