# m038-structured-empty-else-fold

Found while continuing structured control-flow fuzzing after suppressing the
known `not.b32`, `bfind.u32`, `mul24.*`, `mul.hi.*`, `bfi.b32`,
`bmsk.clamp.b32`, `dp2a.*`, and `mad24.*` families from earlier runs.

The original saved fuzzer program was:

```text
/tmp/fuzzx-bg-after-m037-20260515T203837Z/div-1778878363-18afd7ad52bc3e25
```

The minimized PTX in `reduced.ptx` keeps the fuzzer ABI and a single real
conditional. The predicate guarding `structured_if_3_then` is false for every
thread, but optimized ptxas behaves as if the untaken then arm changed `%r3`.

## Scalar Trace

```text
%r3  = 51731 >> 29 = 0
%r7  = 570701863 | %r3 = 570701863
%r7  = %r7 >> 30 = 0
%r8  = 35274 >> 22 = 0
%r17 = (%r10 != 128) ? 0xffffffff : 0
%p28 = %r17 < %r7 = false
```

Because `%p28` is false, the `structured_if_3_then` arm is not executed and
`%r3` should remain `0`. `ptxas -O0` stores `0` in output slot 3 for every
thread. Optimized ptxas stores `0x00000021`, matching the untaken
`xor.b32 %r3, %r0, 1` arm with `in_n = 32`.

Forcing either arm of `structured_if_3`, or deleting the untaken then-arm
`xor.b32`, removes the bug. The reduced repro has no `dp2a`, `mad24`, `cnot`,
`clz`, `bfe`, video op, or signed divide/remainder. This points to structured
conditional branch/control-flow lowering around an always-false predicate and
a non-empty then arm with an empty else.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-15 with CUDA Toolkit 13.2 Update 1 ptxas, the
latest NVIDIA CUDA Toolkit listed on NVIDIA's CUDA Toolkit Archive on
2026-05-15:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_XOR=1`; this reduced
testcase requires the generated `xor.b32` then arm.
