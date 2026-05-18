# m055-prmt-reg-control-eq-fold

Found during a PRMT-mode/register-control side sweep with packed add and
register bitfield generation disabled:

```text
/tmp/fuzzx-bg-prmt-no-packed-1779082682/div-1779083241-18b092c02fb4f647
```

The same 50k sweep found three more candidates that appear to be in this same
PRMT register-control/equality-fold family:

```text
/tmp/fuzzx-bg-prmt-no-packed-1779082682/div-1779083303-18b092c02fb55e6c
/tmp/fuzzx-bg-prmt-no-packed-1779082682/div-1779083304-18b092c02fb5604e
/tmp/fuzzx-bg-prmt-no-packed-1779082682/div-1779083315-18b092c02fb5729f
```

## Scalar Trace

For the checked-in CUDA reproducer:

```text
input = 0x0b1fcdb5
lane = 18
n = 32
t0 = prmt.b32 input, input, 0xa589
ctrl = t0 & 0xffff
t1 = prmt.b32 28, 0xffff, ctrl
tmp = lane
cmp = (t1 == (n << 31))
if cmp:
    tmp = ~n
out = tmp
```

The correct `-O0` output is `0xffffffdf`. Affected optimized ptxas stores
`0x00000012`, as if the equality against the register-control `prmt.b32`
result folded to false for this lane.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-18 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued fuzzing past this family, use `DIV_DISABLE_REG_PRMT=1`.
