# w91 — `llvm.experimental.constrained.ldexp.<fty>.i64` silently truncates exponent (strict variant of #011)

Component: SelectionDAG/LegalizeDAG (generic — observable on x86-64)

## Source

`llvm/lib/CodeGen/SelectionDAG/LegalizeDAG.cpp:2572` (in `SelectionDAGLegalize::expandLdexp`):

```cpp
if (Node->getOpcode() == ISD::STRICT_FLDEXP) // TODO
  return SDValue();
```

`expandLdexp()` bails for `STRICT_FLDEXP`. The caller at line 3941-3956:

```cpp
case ISD::FLDEXP:
case ISD::STRICT_FLDEXP: {
  EVT VT = Node->getValueType(0);
  RTLIB::Libcall LC = RTLIB::getLDEXP(VT);
  // Use the LibCall instead, it is very likely faster
  // FIXME: Use separate LibCall action.
  if (DAG.getLibcalls().getLibcallImpl(LC) != RTLIB::Unsupported)
    break;          // <-- falls through to ConvertNodeToLibcall

  if (SDValue Expanded = expandLdexp(Node)) { ... }
  break;
}
```

falls through to the default `ConvertNodeToLibcall` path at line 5031-5035:

```cpp
case ISD::FLDEXP:
case ISD::STRICT_FLDEXP:
  ExpandFPLibCall(Node, RTLIB::LDEXP_F32, RTLIB::LDEXP_F64, RTLIB::LDEXP_F80,
                  RTLIB::LDEXP_F128, RTLIB::LDEXP_PPCF128, Results);
  break;
```

Unlike the adjacent `FPOWI`/`STRICT_FPOWI` case at line 5074-5084, **no
`ExponentHasSizeOfInt` check is performed**. The strict-fp variant therefore
exhibits the same silent truncation as bug #011, plus the additional concern
that `expandLdexp()` itself (which on AMDGPU and some targets produces the
inline scale-up/down sequence) is unreachable for strictfp.

## x86-64 runtime evidence

```ll
target triple = "x86_64-unknown-linux-gnu"
declare double @llvm.experimental.constrained.ldexp.f64.i64(double, i64, metadata, metadata)
define double @strict_ldexp_i64(double %x, i64 %e) strictfp {
  %r = call double @llvm.experimental.constrained.ldexp.f64.i64(
        double %x, i64 %e,
        metadata !"round.tonearest", metadata !"fpexcept.strict")
  ret double %r
}
```

`llc -mtriple=x86_64-unknown-linux-gnu` emits:

```asm
strict_ldexp_i64:
  pushq  %rax
  callq  ldexp@PLT
  popq   %rax
  retq
```

- glibc `ldexp` has signature `double ldexp(double, int)`. On AMD64 SysV the
  exponent is read from `%edi` (low 32 bits).
- Our caller has the i64 exponent in `%rdi` (high 32 bits live garbage from
  the user-supplied i64 value).
- No sign-extension, no truncation, no `MOVSX %edi,%edi`, no diagnostic.

Concretely, calling `strict_ldexp_i64(1.0, 0x100000020LL)` will pass `0x20` as
the exponent — i.e. `ldexp(1.0, 32) = 4294967296.0` — silently discarding the
high 32 bits the user provided. This is the same behavior as #011 but on the
strict-fp path, which is what users in IEEE-strict mode actually rely on for
exception semantics.

## Why this is distinct from bug #011

- Bug #011 covers non-strict `llvm.ldexp.f64.i64`.
- This bug covers `llvm.experimental.constrained.ldexp.f64.i64` (strictfp).
- The code paths are separately gated:
  - Non-strict: hits `expandLdexp()` only if libcall unsupported; otherwise
    falls through to `ConvertNodeToLibcall` (5031-5035).
  - Strict: `expandLdexp()` is **always skipped** (2572 TODO bail), and the
    `ConvertNodeToLibcall` path lacks the FPOWI-style sizeof(int) guard.
- Fix is to mirror the guard from FPOWI (5074-5084) at the `case ISD::FLDEXP /
  ISD::STRICT_FLDEXP` switch arm, ideally factored into a helper used by both.

## Fix sketch

```cpp
case ISD::FLDEXP:
case ISD::STRICT_FLDEXP: {
  unsigned Offset = Node->isStrictFPOpcode() ? 1 : 0;
  if (DAG.getLibInfo().getIntSize() !=
      Node->getOperand(1 + Offset).getValueType().getSizeInBits()) {
    DAG.getContext()->emitError("LDEXP exponent does not match sizeof(int)");
    Results.push_back(DAG.getPOISON(Node->getValueType(0)));
    if (Node->isStrictFPOpcode())
      Results.push_back(Node->getOperand(0));
    break;
  }
  ExpandFPLibCall(Node, RTLIB::LDEXP_F32, ...);
  break;
}
```
