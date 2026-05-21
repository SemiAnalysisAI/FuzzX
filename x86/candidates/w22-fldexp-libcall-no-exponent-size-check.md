# w22 — FLDEXP scalar libcall expansion silently truncates wide exponent

## Source

`llvm/lib/CodeGen/SelectionDAG/LegalizeDAG.cpp` lines **5031-5035**:

```cpp
case ISD::FLDEXP:
case ISD::STRICT_FLDEXP:
  ExpandFPLibCall(Node, RTLIB::LDEXP_F32, RTLIB::LDEXP_F64, RTLIB::LDEXP_F80,
                  RTLIB::LDEXP_F128, RTLIB::LDEXP_PPCF128, Results);
  break;
```

Compare with adjacent `FPOWI` handling at lines **5074-5084**:

```cpp
bool ExponentHasSizeOfInt =
    DAG.getLibInfo().getIntSize() ==
    Node->getOperand(1 + Offset).getValueType().getSizeInBits();
if (!ExponentHasSizeOfInt) {
  DAG.getContext()->emitError("POWI exponent does not match sizeof(int)");
  Results.push_back(DAG.getPOISON(Node->getValueType(0)));
  break;
}
```

`SoftenFloatRes_ExpOp` in `LegalizeFloatTypes.cpp:750-758` performs the same
guard for both ldexp and powi, but the scalar libcall expansion path for
`FLDEXP` does **not** — it just calls `ldexp(double, int)` /
`ldexpl(long double, int)` with whatever integer SDValue the IR holds.

## Reasoning

`llvm.ldexp.<fty>.<ity>` permits any integer for the exponent (i8, i16, i32,
i64, etc.). On x86_64 the libcall `ldexp` / `ldexpf` / `ldexpl` takes a 32-bit
`int`. When the IR exponent is `i64`, codegen emits a tail call without any
truncation, sign-extension, or error — `rdi` holds the 64-bit value and the
callee reads only `edi`, silently truncating bits 32-63. For any exponent
outside `[INT_MIN, INT_MAX]` the call returns a totally wrong result with no
diagnostic. FPOWI guards against exactly this; FLDEXP forgot.

## IR repro

```llvm
declare double @llvm.ldexp.f64.i64(double, i64)
define double @t(double %a, i64 %e) {
  %r = call double @llvm.ldexp.f64.i64(double %a, i64 %e)
  ret double %r
}
```

```
llc -mtriple=x86_64 ldx.ll -o -
```

Emits:

```
t:
        jmp     ldexp@PLT     # TAILCALL — i64 in %rdi, ldexp reads only %edi
```

Same problem for `fp128`/`x86_fp80`/`<2 x double>+<2 x i64>`:

```llvm
declare fp128 @llvm.ldexp.f128.i64(fp128, i64)
define fp128 @t(fp128 %a, i64 %e) {
  %r = call fp128 @llvm.ldexp.f128.i64(fp128 %a, i64 %e)
  ret fp128 %r
}
```

emits `jmp ldexpl@PLT` — same silent truncation.

Compare to `llvm.powi.f64.i64`, which errors:

```
error: POWI exponent does not match sizeof(int)
```

## Expected wrong outcome

For `e = ((long)INT_MAX) + 5`, the i64 is `0x80000004` in low 32 bits. The
caller intended `ldexp(a, 0x80000004)` (a huge positive exponent → overflow
to +Inf). The libcall instead sees `int = (int)0x80000004 = INT_MIN+4`,
producing 0.0 (underflow). Off by infinity.

## Fix

Mirror the FPOWI guard: either emit an error, or insert a TRUNCATE /
SIGN_EXTEND to make the exponent match `sizeof(int)` before the libcall.

