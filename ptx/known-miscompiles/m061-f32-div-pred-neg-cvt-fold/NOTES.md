# m061-f32-div-pred-neg-cvt-fold

Found while fuzzing uniform vector `ldu.global` generation after suppressing
the earlier scalar-16 families:

```text
divergences/active-20260518-233930-continued-vector-ldu-suppressed/div-1779147684-18b0cd4a04ec1f36
```

The original hit did not depend on `ldu.global`; the live mismatch reduced to
a floating-point value chain feeding a skipped predicated unary operation.

## Scalar Trace

For `reduced.ptx`, every lane computes:

```text
f0 = 33.0
f1 = 33.0
f3 = div.approx.ftz.f32 f0, f1   # approximately 1.0
r15 = cvt.rzi.s32.f32 f3         # 1
f0 = cvt.rn.f32.u32 r15          # 1.0
f1 = f0
p0 = true
@!p0 f1 = neg.f32 f0             # not executed
r5 = cvt.rzi.s32.f32 f1          # should be 1
```

`ptxas -O3` stores `1` for all lanes. `ptxas -O0` stores `0`, as if the
fallback `mov.f32` into `%f1` were dropped when the skipped predicated
`neg.f32` is present.

This reproduced on 2026-05-19 with CUDA Toolkit 13.0 ptxas:

```text
release 13.0, V13.0.88
cuda_13.0.r13.0/compiler.36424714_0
```

For continued fuzzing past this family, use
`DIV_DISABLE_PREDICATED_UNARY=1`.
