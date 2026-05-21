# X86: strict-fp `constrained.fcmp[s].v2f128` ICE — "Do not know how to expand the result of this operator!"

## Summary

`llvm.experimental.constrained.fcmp.v2f128` (and the signaling `fcmps` variant) crash the X86 backend during type legalization. The non-strict equivalent (`fcmp olt <2 x fp128>`) compiles fine — it gets scalarized to two libcalls.

The DAG type legalizer's vector-result expander does not know how to expand STRICT_FSETCC / STRICT_FSETCCS when the operand type is a vector of fp128 (needs to scalarize via libcall while preserving the chain).

## Reproducer (minimal)

```llvm
; t51c.ll
define <2 x i1> @t(<2 x fp128> %a, <2 x fp128> %b) #0 {
  %r = call <2 x i1> @llvm.experimental.constrained.fcmp.v2f128(<2 x fp128> %a, <2 x fp128> %b, metadata !"olt", metadata !"fpexcept.strict")
  ret <2 x i1> %r
}
declare <2 x i1> @llvm.experimental.constrained.fcmp.v2f128(<2 x fp128>, <2 x fp128>, metadata, metadata)
attributes #0 = { strictfp }
```

The signaling variant `constrained.fcmps.v2f128` crashes identically.

## Command

```
llc -O2 -mtriple=x86_64-linux-gnu t51c.ll -o -
```

No special features required.

## Crash

```
LLVM ERROR: Do not know how to expand the result of this operator!
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ ...
Stack dump:
1. Running pass 'Function Pass Manager' on module 't51c.ll'.
2. Running pass 'X86 DAG->DAG Instruction Selection' on function '@t'

Frames:
  llvm::report_fatal_error(llvm::Twine const&, bool) + 437
  (anon)  -> SplitVecRes/ScalarizeVecRes_* dispatch in LegalizeVectorTypes
  llvm::SelectionDAG::LegalizeTypes() + 1399
```

Frame 9 (`0x0000000004484018`) is in the vector-result type legalization switch, which has no case for `STRICT_FSETCC`/`STRICT_FSETCCS` when the operand type is fp128 (needs SoftenFloatOperand-style libcall scalarization for strict-fp).

## Comparison

- `fcmp olt <2 x fp128> %a, %b` (non-strict)        — works (scalarizes to two `__lttf2` libcalls).
- `llvm.experimental.constrained.fcmp.f128` (scalar) — works (one `__lttf2` libcall, chain preserved).
- `llvm.experimental.constrained.fcmp.v2f128`        — **crashes** (vec scalarizer missing strict-fp case).
- `llvm.experimental.constrained.fadd.v2f128`        — works (likely via splitting then per-lane libcall).

So the gap is specifically `STRICT_FSETCC[S]` on vector-of-soft-FP types in the type legalizer.

## Root cause (hypothesis)

`lib/CodeGen/SelectionDAG/LegalizeVectorTypes.cpp` — the dispatcher `DAGTypeLegalizer::ScalarizeVectorResult` or `SplitVectorResult` is missing a `STRICT_FSETCC`/`STRICT_FSETCCS` arm for operands that need to be expanded (i.e. fp128 softened by `ExpandFloatRes`). The non-strict `SETCC` arm exists; the strict version needs to be added with chain bookkeeping.
