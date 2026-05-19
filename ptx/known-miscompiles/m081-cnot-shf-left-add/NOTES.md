# m081-cnot-shf-left-add

Found while continuing the CUDA 13.2.78 sweep after adding helper-call and half
compare prologue coverage:

```text
divergences/active-20260519-114049-ptxas-13.2.78-post-f16-compare-select/div-1779190933-18b0f4a693b211d1
```

The saved fuzzer program reduced to a straight-line `cnot.b32` feeding the
first source of `shf.l.wrap.b32`, followed by an add:

```ptx
mov.u32        %r1, %tid.x;
cnot.b32       %r2, %r1;
shf.l.wrap.b32 %r3, %r2, %r1, 18;
add.u32        %r4, %r3, %r1;
```

For nonzero `%tid.x`, `cnot.b32` produces zero, so the correct result is:

```text
(tid << 18) + tid
```

For `tid = 1`, `ptxas -O0` stores the expected `0x00040001`.
`ptxas -O3` stores `0xfffc0001`, as if the shifted contribution came from the
negated value instead of the original left-shifted source. `tid = 0` also
differs: `-O0` stores `0x00000000`, while `-O3` stores `0xfffc0001`.

This is likely the same optimizer family as
[`m021-cnot-funnel-add`](../m021-cnot-funnel-add/NOTES.md), which hit
`shf.r.wrap.b32`; this repro shows the left-wrap form is also affected.
Replacing the `cnot.b32` with a literal zero, `not.b32`, or `neg.s32` did not
reproduce the mismatch in the hand-sliced kernels.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m081-cnot-shf-left-add/reduced.ptx \
  known-miscompiles/m081-cnot-shf-left-add/input.bin
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

For continued fuzzing past this family, use `DIV_DISABLE_CNOT=1`.
