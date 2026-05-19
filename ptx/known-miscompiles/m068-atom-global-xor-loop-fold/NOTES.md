# m068-atom-global-xor-loop-fold

Found while replaying the seed that produced m067 after suppressing
`atom.global.dec.u32`:

```text
divergences/active-20260519-073020-ptxas-13.2.78-post-m067-smoke/div-1779174047-18b0e49b5de14514
```

The reduced program loops over a per-thread output slot. Some lanes update the
loop-carried `%r6` value with a `bmsk.clamp.b32` result, then the loop stores
and atomically xors that value:

```ptx
@%p2 bmsk.clamp.b32 %r6, 15, 3;

st.global.u32       [%rd8 + 12], %r6;
mov.u32            %r10, %r0;
atom.global.xor.b32 %r7, [%rd8 + 12], %r10;
ld.global.u32      %r10, [%rd8 + 12];
add.u32            %r7, %r7, %r10;
```

`%r0` is `0x6655`. At `-O0`, affected lanes write the loop-updated mask value
`0x00038000 ^ 0x6655 == 0x0003e655` into output slot 3. At `-O3`, optimized
ptxas writes the original input word xor `0x6655` instead, as if it ignored the
loop-carried `%r6` update before the `atom.global.xor.b32` roundtrip.

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas \
target/release/fuzzx-diff-test \
  known-miscompiles/m068-atom-global-xor-loop-fold/reduced.ptx \
  known-miscompiles/m068-atom-global-xor-loop-fold/input.bin
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

For continued fuzzing past this family, use `DIV_DISABLE_GLOBAL_ATOMIC_XOR=1`.
