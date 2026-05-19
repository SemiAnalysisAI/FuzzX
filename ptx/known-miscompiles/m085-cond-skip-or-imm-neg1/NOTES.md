# m085-cond-skip-or-imm-neg1

ptxas `-O3` produces the wrong value when a predicate-guarded skip
straddles a write to a register that was previously initialised to the
`1.0f` bit pattern (`0x3f800000`). The optimiser appears to fold the
conditional as if the guard were "always true" — i.e. it eliminates the
guarded write — and leaves the `0x3f800000` initialiser as the live
value, even though the predicate is set from runtime parameter data
and may be either true or false.

Found while continuing the CUDA 13.2.78 sweep after `DIV_DISABLE_RICH_HELPER_CALLS=1`
(m083 suppressed) and `DIV_DISABLE_I32_BOUNDARY_IMMS=1`:

```text
divergences/active-20260519-185043-m084-cvtpack-suppressed/div-1779216671-18b10c1c52a811e2
```

## Reduced repro (19 lines)

```ptx
.version 8.8
.target sm_103
.address_size 64

.visible .entry fuzz_kernel(.param .u64 in_ptr, .param .u64 out_ptr, .param .u32 in_n)
{
    .reg .pred  %p<1>;
    .reg .b32   %r<2>;
    .reg .b64   %rd<2>;
    ld.param.u64    %rd0, [out_ptr];
    ld.param.u32    %r0, [in_n];
    mov.b32         %r1, 0x3f800000;
    setp.eq.u32     %p0, %r0, 42;
    @%p0 bra        done;
    or.b32          %r1, 4294967295, %r0;
done:
    cvta.to.global.u64 %rd0, %rd0;
    st.global.u32   [%rd0 + 4], %r1;
}
```

The fuzzer harness launches with `in_n == 32`, so `%p0 = (32 == 42) =
false`, the `bra` is **not** taken, and the `or.b32 %r1, -1, %r0` runs
and sets `%r1 = 0xffffffff`. The expected output is therefore
`0xffffffff` in slot 1.

| ptxas | tid 0 slot 1 |
| --- | --- |
| 13.2.78 `-O0` | `0xffffffff` (correct) |
| 13.2.78 `-O3` | `0x3f800000` (the initial `mov.b32` value) |
| 13.0.88 `-O0` | `0xffffffff` (correct) |
| 13.0.88 `-O3` | `0x3f800000` (same bug) |

## What it depends on

The bug is highly specific to the **`0x3f800000` initialiser**. Tested
with every "obvious" replacement constant; only `0x3f800000` fires:

| init `mov.b32 %r1, X` | `-O3` slot 1 |
| --- | --- |
| `0x3f800000` (1.0f) | `0x3f800000` (BUG) |
| `0x40000000` (2.0f) | `0xffffffff` (ok) |
| `0xbf800000` (-1.0f) | `0xffffffff` (ok) |
| `0x3f7fffff` (1.0f - ULP) | `0xffffffff` (ok) |
| `0x3f800001` (1.0f + ULP) | `0xffffffff` (ok) |
| `0x3f000000` (0.5f) | `0xffffffff` (ok) |
| `0x7f800000` (+inf) | `0xffffffff` (ok) |
| `0x7fc00000` (qNaN) | `0xffffffff` (ok) |
| `0x00000000` (0.0f) | `0xffffffff` (ok) |
| `0xffffffff` (all-1s) | `0xffffffff` (ok) |
| `0x12345678` (random) | `0xffffffff` (ok) |
| `0xdeadbeef` (random) | `0xffffffff` (ok) |

The wrong-side instruction is also flexible — `or.b32 %r1, -1, _`,
`not.b32 %r1, 0`, and `mov.u32 %r1, -1` all reproduce. The trigger is
the combination of:

1. A `mov.b32` of `0x3f800000` into the target register before the
   branch.
2. A predicate-guarded `@%p bra done;` (with `%p` set by `setp` against
   a kernel parameter so the optimiser cannot fold it).
3. An unconditional join-block read of the target register.

The bug therefore looks like an optimiser pass that special-cases the
canonical `1.0f` bit-pattern in the integer/predicate domain and
mis-folds the conditional skip on top of it.

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

Reproduced on 2026-05-19 with:

* CUDA Toolkit 13.2 Update 1 ptxas: `release 13.2, V13.2.78`,
  build `cuda_13.2.r13.2/compiler.37668154_0`
* CUDA Toolkit 13.0 ptxas: `release 13.0, V13.0.88`,
  build `cuda_13.0.r13.0/compiler.36424714_0`

## Fuzzer-side mitigation and suppressor

The fuzzer's bf16/tf32 prologue used to emit
`mov.b32 %r1, 0x3f800000;` into an *output* register, which is what
exposed this bug through the differential oracle. After commit
`8c55762` ("Move bf16/tf32 prologue setup into scratch regs"), the
1.0f bit pattern is written to a scratch slot instead, so the bug no
longer reaches the per-thread output store.

`DIV_DISABLE_BF16_TF32_CVT=1` is the broad env-flag suppressor; it
removes the prologue's `mov.b32 ..., 0x3f800000` entirely. After the
scratch-reg fix the broad suppressor is no longer needed in the
fuzzer.
