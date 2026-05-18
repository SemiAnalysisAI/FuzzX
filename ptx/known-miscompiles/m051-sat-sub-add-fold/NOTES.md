# m051-sat-sub-add-fold

Found during a focused sweep after adding signed saturating add/sub generation.

The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-sat-popc-J2lt5UV0/div-1779071838-18b0886608e18139
```

The reducer shrank the testcase from 63 to 46 lines.

## Scalar Trace

For each thread:

```text
input = in[tid]
a = tid + input
b = a + input
c = sub.sat.s32(b, input)
out = c + input
```

The optimized compiler appears to fold `sub.sat.s32(b, input) + input` back to
`b`. That rewrite is only valid when the signed saturating subtraction does not
saturate. For lane 2 in the checked-in input, `b = 0xc92ae480` and
`input = 0x6495723f`; the signed subtraction saturates to `INT_MIN`, so the
correct output is `0xe495723f`. Optimized ptxas stores `0xc92ae480`.

This is a new saturating-arithmetic family rather than the earlier wrapped
subtraction folds: the reduced testcase has no body `lop3`, `prmt`, `mul`,
`mul.wide`, bitfield instruction, predicate, branch, loop, or 64-bit scratch
ALU operation. It only needs `add.u32` and `sub.sat.s32`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-18 with CUDA Toolkit 13.0 nvcc/ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued fuzzing past this family, use `DIV_DISABLE_SAT_ARITH=1`.
