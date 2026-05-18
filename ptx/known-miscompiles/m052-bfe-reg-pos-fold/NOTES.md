# m052-bfe-reg-pos-fold

Found during a focused sweep after adding register-valued bitfield pos/len
operands. The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-reg-bitfield-1779080785/div-1779080800-18b0908c4edeb33a
```

The reducer produced a 56-line PTX file. A manual live-value pass reduced the
checked-in PTX to a single `bfe.s32` with a register start position.

## Scalar Trace

```text
n = 32
pos = 131072 + 3610147258 = 3610278330
pos8 = pos & 0xff = 250
len = 9
out = bfe.s32 n, pos, len
```

For signed `bfe`, an out-of-range start position fills the destination with the
replicated sign bit from the source operand. Since `n` is `32`, that sign bit is
zero and the correct result is `0x00000000`.

`ptxas -O0` stores `0x00000000`. Affected optimized ptxas stores
`0xffffffff`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-18 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued fuzzing past this family, use `DIV_DISABLE_REG_BITFIELD=1`.
