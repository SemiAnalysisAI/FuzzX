# m067-atom-global-dec-loop-fold

Found while fuzzing CUDA 13.2.78 `ptxas` after suppressing global reductions,
register-operand bitfield forms, and the known `prmt` value-flow families:

```text
divergences/active-20260519-065003-ptxas-13.2.78-post-m066-nored-noregbitfield/div-1779173418-18b0e49b5de14514
```

The reduced program initializes `%r0` from constant memory to `0x6655`, then
loops through a per-thread global store/atomic/load sequence:

```ptx
st.global.u32       [%rd8 + 12], %r6;
mov.u32            %r10, %r0;
atom.global.dec.u32 %r7, [%rd8 + 12], %r10;
ld.global.u32      %r10, [%rd8 + 12];
add.u32            %r7, %r7, %r10;
```

The final live-out is controlled by:

```ptx
setp.le.s32   %p7, 0, %r3;
@!%p7 shr.u32 %r1, %r0, 0;
```

At `-O0`, six affected lanes execute the final predicated move and store
`0x00006655` in output slot 1. At `-O3`, optimized ptxas leaves the previous
input value `0x5ab65edd` in that slot for those lanes, as if the loop-carried
state around the `atom.global.dec.u32` roundtrip changed the final predicate or
its value flow.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m067-atom-global-dec-loop-fold/reduced.ptx \
  known-miscompiles/m067-atom-global-dec-loop-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 6/32 tids differ, 6/128 u32 slots differ
```

This reproduced on 2026-05-19 with CUDA Toolkit 13.2 Update 1 ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_GLOBAL_ATOMIC_DEC=1`.
