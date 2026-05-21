# X86: strict-fp constrained arithmetic on bf16 ICE — "Do not know how to soft promote this operator's result!"

## Summary

Strict-fp arithmetic on scalar bf16 (e.g. `llvm.experimental.constrained.fadd.bf16`, `.sqrt.bf16`) crashes the X86 backend during type legalization. The SoftPromoteHalfResult legalizer has no case for the strict-fp arithmetic opcodes when the result type is bf16.

Distinct from the strict-fp `fcmp` bf16 crash (operand soft-promote) — this one fires on the result side.

## Reproducer

```llvm
; t42a.ll
define bfloat @t(bfloat %a, bfloat %b) #0 {
  %r = call bfloat @llvm.experimental.constrained.fadd.bf16(bfloat %a, bfloat %b, metadata !"round.dynamic", metadata !"fpexcept.strict")
  ret bfloat %r
}
declare bfloat @llvm.experimental.constrained.fadd.bf16(bfloat, bfloat, metadata, metadata)
attributes #0 = { strictfp }
```

Also reproduces with `constrained.sqrt.bf16` (unary) and likely any strict-fp op that returns bf16. The `fpext bf16 -> f32` strict variant (`constrained.fpext.f32.bf16`) is in the same family.

## Command

```
llc -O2 -mtriple=x86_64-linux-gnu t42a.ll -o -
```

No special features required — fails with default mattr.

## Crash

```
LLVM ERROR: Do not know how to soft promote this operator's result!
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ ...
Stack dump:
1. Running pass 'Function Pass Manager' on module 't42_bf16_more.ll'.
2. Running pass 'X86 DAG->DAG Instruction Selection' on function '@t'

Frames:
  llvm::report_fatal_error(llvm::Twine const&, bool) + 437
  (anon)  -> DAGTypeLegalizer SoftPromoteHalfResult dispatch
  llvm::SelectionDAG::LegalizeTypes() + 1399
  llvm::SelectionDAGISel::CodeGenAndEmitDAG()
```

Frame 9 (`0x00000000044f2b4e`) corresponds to the result-side `SoftPromoteHalfResult` switch, the dual of the operand-side switch from the fcmp bug.

## Root cause (hypothesis)

`SoftPromoteHalfResult` in `lib/CodeGen/SelectionDAG/LegalizeFloatTypes.cpp` is missing `STRICT_FADD`/`STRICT_FSQRT`/`STRICT_FP_EXTEND` cases. The non-strict equivalents are handled (regular `fadd bfloat` compiles).

Both bf16 strict-fp ICEs (this one + the fcmp candidate) point at the same code region: bf16 soft-promotion forgot to mirror the strict-fp opcodes.
