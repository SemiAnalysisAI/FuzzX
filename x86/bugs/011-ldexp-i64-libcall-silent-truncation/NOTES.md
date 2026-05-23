# 011 — `llvm.ldexp.<fty>.i64` silently truncates exponent to `int` on libcall expansion

Component: SelectionDAG/LegalizeDAG (generic, but observable in x86 pipeline)

## Source

`llvm/lib/CodeGen/SelectionDAG/LegalizeDAG.cpp:5031-5035`

```cpp
case ISD::FLDEXP:
case ISD::STRICT_FLDEXP:
  ExpandFPLibCall(Node, RTLIB::LDEXP_F32, RTLIB::LDEXP_F64, RTLIB::LDEXP_F80,
                  RTLIB::LDEXP_F128, RTLIB::LDEXP_PPCF128, Results);
  break;
```

The adjacent `FPOWI` handling (lines 5074-5084) and `SoftenFloatRes_ExpOp`
both perform:

```cpp
bool ExponentHasSizeOfInt =
    DAG.getLibInfo().getIntSize() ==
    Node->getOperand(1 + Offset).getValueType().getSizeInBits();
if (!ExponentHasSizeOfInt) {
  DAG.getContext()->emitError("POWI exponent does not match sizeof(int)");
  ...
}
```

`FLDEXP` forgot this guard. `llvm.ldexp.<fty>.<ity>` accepts any integer
exponent width in IR, but the libcall `ldexp` (and `ldexpf`, `ldexpl`)
takes a 32-bit C `int`. When the IR exponent is `i64`, codegen emits a
bare tail call:

```
t:
        jmp     ldexp@PLT       # TAILCALL
```

`rdi` holds the full 64-bit value; the callee reads only `edi` — the upper
32 bits are silently dropped.

## Runtime demonstration

`runner.c` calls `t(1.0, 0x80000004)`. The exponent is a large positive
i64 (~2.1 billion); the true `ldexp(1.0, that)` is `+Inf`. Because the
low 32 bits of `0x80000004` are interpreted as `int = INT_MIN + 4`
(very negative), the libcall returns `0.0`:

```
ldexp(1.0, 2147483652) -> 0 (expected +Inf; libcall reads only low 32 bits)
FAIL — i64 exponent silently truncated to 32 bits
```

So a positive-overflow input yields a (very-negative-underflow)
zero result — off by infinity in the wrong direction.

Same problem at `f32` / `x86_fp80` / `fp128`.

## Comparison: POWI errors loudly

```ll
declare double @llvm.powi.f64.i64(double, i64)
```

Compiling this errors out: `error: POWI exponent does not match sizeof(int)`.
LDEXP silently miscompiles instead.

## Fix

Mirror the FPOWI guard: emit an error, or insert a `TRUNCATE` /
`SIGN_EXTEND` to bring the exponent to the libcall's `sizeof(int)` width
before the call.

## Disposition

Tried, dropped. The FPOWI-style guard in LegalizeDAG produces false
positives on RV64: type legalization promotes a user-written i32
exponent to i64 (since i32 isn't a legal SDAG type on RV64), so by the
time `ConvertNodeToLibcall` runs there's no way to tell a real i64 IR
exponent apart from a legalized-from-i32 one. A correct fix has to run
in `SelectionDAGBuilder` on the IR-level type, which is more invasive
than the bug warrants — the silent truncation matches C `ldexp`'s
`int` parameter, no in-tree producer of LLVM IR (clang, MLIR, …)
emits `llvm.ldexp.<fty>.i64`, and PR
https://github.com/llvm/llvm-project/pull/199177 was closed.

## Files
- `repro.ll`  — i64-exponent ldexp
- `runner.c`  — drives with `(int64_t)0x80000004`
- `cmd.sh`    — show asm + run; non-zero exit reproduces the bug
