# m087-redux-xor-guarded-f64-cvt

ptxas `-O3` produces the wrong value when an `f64`-to-`s32` conversion that
should write `%r3` is guarded by a `setp.ne.u32 %p, %r6, 0; @%p bra done;`
where `%r6` came from a `redux.sync.xor.b32` reduction. At runtime the
predicate is false (the reduction result is 0), so the f64-cvt arm should
run; `-O3` instead skips it and leaves `%r3` at the value an earlier
predicated `mov.b64 {%r3, %r9}, %rd7;` may or may not have written.

Found during the first fuzzer run after pushing the 64-bit shared-atomic
prologue feature:

```text
divergences/active-20260519-202110-u64-atom-active/div-1779224899-18b1110becaa620b
```

## Reduced repro (44 lines)

```ptx
.version 8.8
.target sm_103
.address_size 64
.const .align 16 .b8 fuzzx_const[64] = { /* the standard 64-byte ramp */ };

.func (.reg .b32 ret0) fuzzx_helper(.reg .b32 a, .reg .b32 b)
{
    add.u32         ret0, a, b;
    ret;
}

.visible .entry fuzz_kernel(.param .u64 in_ptr, .param .u64 out_ptr, .param .u32 in_n)
{
    .reg .pred  %p<18>;
    .reg .b16   %h<4>;
    .reg .b32   %r<13>;
    .reg .b64   %rd<10>;
    .reg .f64   %fd<4>;
    .shared .align 16 .b8 fuzzx_shared[512];
    ld.param.u64    %rd1, [out_ptr];
    add.u32         %r0, %r0, %r9;
    createpolicy.range.global.L2::evict_last.L2::evict_first.b64 %rd7, [%rd6], 64, 128;
    max.f16         %h2, %h2, %h0;
    max.f16x2       %r1, %r1, %r2;
    add.u32         %r0, %r0, %r9;
    redux.sync.xor.b32 %r6, %r3, 0xffffffff;
    mov.u64       %rd6, fuzzx_shared;
    not.b32       %r1, %r6;
    @%p6 mov.b64 {%r3, %r9}, %rd7;
    setp.ne.u32   %p9, %r6, 0;
    @%p9 bra   block_2;
    and.b32       %r9, %r1, 1023;
    cvt.rn.f64.u32 %fd1, %r9;
    min.f64       %fd3, %fd0, %fd1;
    cvt.rzi.s32.f64 %r3, %fd3;
    cvt.rn.f64.u32 %fd0, %r9;
block_2:
    cvta.to.global.u64 %rd4, %rd1;
    st.global.u32   [%rd4 + 12], %r3;
}
```

For thread 0 of a 32-thread launch with `%tid.x = 0`, the `redux.sync.xor`
input `%r3` is uninitialised so the redux behavior is technically
implementation-defined, but `-O0` consistently lets the f64-cvt arm
execute and `-O3` consistently skips it — both deterministic across
5 recompile+run cycles.

| ptxas | tid 0 slot 3 |
| --- | --- |
| 13.2.78 `-O0` | `0x000003ff` |
| 13.2.78 `-O3` | `0x00000000` |
| 13.0.88 `-O0` | `0x000003ff` |
| 13.0.88 `-O3` | `0x00000000` |

## Reproduce

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m087-redux-xor-guarded-f64-cvt/reduced.ptx \
  known-miscompiles/m087-redux-xor-guarded-f64-cvt/input.bin
```

Observed result:

```text
DIVERGES (deterministic) — 1/32 tids differ, 1/128 u32 slots differ
```

(The fuzzer-saved `original.ptx` shows the same bug with 24/32 tids
differing — the line-by-line reducer drives the program down a path
where only thread 0 of the warp observes the mismatch.)

## Family

Same shape as m085 — predicate-guarded skip over a write to the same
register that the join block reads. m085 is specific to the
`0x3f800000` initial value seeded by the bf16/tf32 prologue; m087's
"prior" value comes from a different source (a runtime-conditional
`mov.b64 {%r3, %r9}, %rd7` with an uninitialised predicate, plus a
`redux.sync.xor.b32` upstream of the guard). Both arms of m087's
trigger are well-defined PTX given consistent inputs; the optimiser
appears to fold the guard predicate to "always skip" regardless.

The bug was rediscovered minutes after enabling the 64-bit shared
atomic prologue — but neither the 64-bit shared atomics nor the
signed `redux.sync.add.s32` form (recent additions) are required;
both can be deleted from the reduced repro and the bug still fires.
So it is a pre-existing ptxas bug that the new generator surface
happened to expose.

## Suppressor

No targeted suppressor added. The bug is rare enough at the current
suppressor level (~1 hit per 25 minutes) that we accept the noise.
A targeted fix would require disabling either the `redux.sync.xor`
prologue (heavy) or the predicated `mov.b64 {tuple}` random body
instruction.
