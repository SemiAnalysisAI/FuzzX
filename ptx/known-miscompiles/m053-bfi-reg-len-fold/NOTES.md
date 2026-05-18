# m053-bfi-reg-len-fold

Found during the same register-bitfield sweep as
`m052-bfe-reg-pos-fold`. Two saved fuzzer programs reduced to the same
register-length `bfi.b32` family:

```text
/tmp/fuzzx-ptx-reg-bitfield-1779080785/div-1779080800-18b0908c4edeb80c
/tmp/fuzzx-ptx-reg-bitfield-1779080785/div-1779080801-18b0908c4edebe18
```

The reducer produced 72-line and 73-line PTX files. A manual live-value pass
reduced the checked-in PTX to one `bfi.b32` with a register length operand.

## Scalar Trace

For the checked-in reproducer:

```text
input = in[tid]
n = 32
len = 25 + 24 = 49
out = bfi.b32 n, input, 6, len
```

`bfi.b32` uses the low 8 bits of the register length. With `len = 49`, the
operation replaces bits 6 through 31 of `input` with low bits from `n`.
For `input = 0xc58c427c`, the correct `-O0` output is `0x0000083c`.

Affected optimized ptxas preserves high bits from the base value and stores
`0xc580083c` for that same input. This is likely the same root cause as the
second saved sweep hit, which also uses a register length operand to `bfi.b32`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-18 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued fuzzing past this family, use `DIV_DISABLE_REG_BITFIELD=1`.
