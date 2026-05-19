# m078-mul-lo-loop-recurrence-fold

Found while continuing the CUDA 13.2.78 sweep after suppressing PRMT, LOP3,
f32 unary, constant memory, generic memory, memory cache ops, global store
roundtrips, MAD carry, high multiply, global reductions, global atomics,
register-operand bitfield forms, wide `subc`, wide `bfi`, and predicated
`mad.lo`:

```text
divergences/active-20260519-ptxas-13.2.78-post-m077-nogen-nostore/div-1779179101-18b0e49b5de188aa
```

The generated 120-line program reduced to a pure scalar recurrence. The
required part is an arbitrary-control-flow loop that repeatedly updates `%r2`
with a low multiply-add and then stores `or.b32 %r1, 32, %r2`:

```ptx
block_3:
    mad.lo.u32    %r2, %r2, %r7, %r1;
    setp.eq.u32   %p8, %r9, 0;
    @%p8 bra   loop_done_9;
    sub.u32         %r9, %r9, 1;
    bra             block_3;
loop_done_9:
    bra             block_4;

block_4:
    or.b32        %r1, 32, %r2;
    setp.eq.u32   %p10, %r10, 0;
    @%p10 bra   exit;
    sub.u32         %r10, %r10, 1;
    bra             block_3;
```

At `-O0`, slot 1 stores the fully iterated recurrence. At `-O3`, 17 lanes
store values shaped like the original input word ORed with `0x20`, such as
`0x94c8d732`, `0xd137caa4`, and `0xbd8094b9`, as if the loop-carried low
multiply recurrence was skipped for those lanes. Replacing the `mad.lo.u32`
with an equivalent `mul.lo.u32` plus `add.u32` still diverges, so this is not
just a MAD-combine issue.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m078-mul-lo-loop-recurrence-fold/reduced.ptx \
  known-miscompiles/m078-mul-lo-loop-recurrence-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 17/32 tids differ, 17/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_MUL_LO=1`.
