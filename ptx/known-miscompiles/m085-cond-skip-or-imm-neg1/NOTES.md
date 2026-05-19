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

## Broader family

The first reproducer used `or.b32 %r1, 0xffffffff, %r0` as the
0xffffffff-producing instruction. After `DIV_DISABLE_I32_BOUNDARY_IMMS=1`
was added (no more immediate `-1`), the fuzzer immediately found two more
divergences with the same output-slot symptom — `O0: 0xffffffff` vs
`O3: 0x3f800000` in slot 1 of every thread — but with different
0xffffffff-producing instructions in the body, including `not.b32 %r, 0`.

So the trigger is not specifically `or.b32 imm,-1`; the common pattern is:

1. The deterministic bf16/tf32 prologue at `lib.rs:11949` seeds
   `mov.b32 %r1, 0x3f800000;` to prepare the `cvt.rn.bf16.f32` chain —
   leaving `0x3f800000` in `%r1`.
2. Random body instructions later overwrite `%r1` with `0xffffffff` (via
   any single-result instruction that lands on the all-ones bit pattern:
   `or.b32 %r, -1, _`, `not.b32 %r, 0`, etc.) inside a small
   uniform-prefix-guarded arm.
3. `-O3` incorrectly dead-store-eliminates the body write, so `%r1` keeps
   the prologue's `0x3f800000` value at the final `st.global.u32 [%rd4+4]`.

The fundamental problem is the bf16/tf32 prologue using output registers
(`%r1`, `%r2`, `%r3`) as bf16 scratch rather than dedicated scratch slots,
which makes the output value depend on whether the body's writes survive
optimization. A long-term fix is to move the prologue's bf16 scratch into
the existing `wide_scratch_hi_reg()` pool; that has not been done in this
commit.

## Suppressor

`DIV_DISABLE_BF16_TF32_CVT=1` removes the prologue line that seeds
`%r1 = 0x3f800000`, which removes the family's wrong-value source. That is
heavier than ideal — it loses all bf16/tf32 conversion coverage from
`3483c72` — but it is the single env flag that gates the family.
