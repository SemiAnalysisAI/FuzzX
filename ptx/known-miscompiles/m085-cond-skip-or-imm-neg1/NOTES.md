# m085-cond-skip-or-imm-neg1

Found while continuing the CUDA 13.2.78 sweep after `DIV_DISABLE_CVT_PACK=1`
(m084 family suppressed) and `DIV_DISABLE_RICH_HELPER_CALLS=1` (m083
suppressed):

```text
divergences/active-20260519-185043-m084-cvtpack-suppressed/div-1779216671-18b10c1c52a811e2
```

`OUTPUT_MISMATCH` (both opt levels compile and launch cleanly; outputs
disagree on all 32 threads). The bug surfaces in the second 32-bit output
slot, `%r1`:

```text
-O0 stores 0xffffffff
-O3 stores 0x3f800000     (the 1.0f bit pattern)
```

## Trigger

The 345-line `reduced.ptx` contains a control-flow tail of the form:

```ptx
block_5:
    cvta.to.global.u64 %rd6, %rd0;
    prefetch.global.L2  [%rd6 + 53];
    setp.eq.u32   %p11, 11, %r7;
    @%p11 bra   block_7;
    bra             block_6;

block_6:
    or.b32        %r1, 4294967295, %r0;
    bra             block_7;

block_7:
    ... reads %r1 ...
```

`%r7` is `mov.u32 %r7, %r0;` where `%r0` is the kernel's runtime `in_n`
parameter — so the optimizer cannot know `%r7` at compile time. Both
control-flow paths converge at `block_7` and the code reads `%r1`.

Earlier in the prologue, `%r1` is initialized via `mov.b32 %r1, 0x3f800000;`
(the 1.0f constant used to seed the f32 bit-preserving helper) and then
mutated by the f16x2 arithmetic prologue. At the join point in `block_7`,
the only path that overwrites `%r1` with `0xffffffff` is the `block_6` body
(the OR with immediate `-1`), which executes when `%r7 != 11`.

In the saved test, `%r7 == 32`, so block_6 should execute and `%r1` should
be `0xffffffff`. `-O0` agrees and stores `0xffffffff`. `-O3` instead stores
the earlier prologue value (`0x3f800000`), as if it had folded the
`block_6` arm out — i.e. assumed `%r7 == 11` and skipped the `or.b32`.

The XOR of the differing slots — `0xffffffff ^ 0x3f800000 = 0xc07fffff` —
matches the prior `%r1` value bleeding through the join. Output slot 1 is
the only one that disagrees.

## Reproduce

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m085-cond-skip-or-imm-neg1/reduced.ptx \
  known-miscompiles/m085-cond-skip-or-imm-neg1/input.bin
```

Observed result:

```text
DIVERGES (deterministic) — 32/32 tids differ, 32/128 u32 slots differ
```

Confirmed deterministic across 5 recompile-and-run cycles per opt level.
Found on 2026-05-19 with `release 13.2, V13.2.78`,
build `cuda_13.2.r13.2/compiler.37668154_0`.

## Suppressor

No targeted suppressor flag was added. The bug requires a fairly specific
shape — a `setp.eq.u32 imm, reg` guarding a single-arm `bra` that joins
back to a block reading the same destination — so it does not dominate the
divergence inbox in the way m083 or m084 did. The line-by-line reducer
arrived at 345 lines and stalled; isolating a sub-100-line repro would
require a manual control-flow rewrite.
