# m063-f32-approx-div-cvt-boundary

Found while fuzzing after suppressing the packed min/max live-out family:

```text
divergences/active-20260519-020300-post-m062/div-1779156227-000000006a0c0c56
```

The original hit did not depend on the vector or uniform global loads that
were active in the run. The live mismatch reduces to an approximate f32 divide
whose result is truncated to an integer.

## Scalar Trace

For tid 20 in `reduced.ptx`, the input word is `0x41028442`, so:

```text
f0 = float(0x41028442 & 1023)  # 66.0
f1 = float((32 & 1023) + 1)    # 33.0
f3 = div.approx.ftz.f32 f0, f1 # approximately 2.0
r1 = cvt.rzi.s32.f32 f3
```

`ptxas -O0` stores `1` for tid 20, while `ptxas -O3` stores `2`. The PTX ISA
documents `div.approx.f32` as a fast approximate divide with up to 2 ulp error,
so this is an exact-output oracle tolerance issue rather than a useful compiler
miscompile. A following `cvt.rzi.s32.f32` can magnify an allowed near-integer
approximation difference into an integer output mismatch.

This reproduced on 2026-05-19 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued exact-output fuzzing past this family, use
`DIV_DISABLE_F32_ARITH=1`.
