# X86: strict-fp constrained.fcmp[s] on bf16 ICE — "Do not know how to soft promote this operator's operand!"

## Summary

`llvm.experimental.constrained.fcmp.bf16` (or `fcmps.bf16`) on scalar or vector bf16 crashes the X86 backend during type legalization (SoftPromoteHalfOperand). The bf16 strict-fp compare is not handled by the soft-promotion legalizer.

The non-strict equivalent (regular `fcmp olt bfloat ...`) compiles fine. The crash occurs with default mattr — no AVX/AVX512BF16 flags are required to trigger it.

## Reproducer

```llvm
; t39_red.ll
define i32 @t(bfloat %a, bfloat %b) #0 {
  %r = call i1 @llvm.experimental.constrained.fcmp.bf16(bfloat %a, bfloat %b, metadata !"olt", metadata !"fpexcept.strict")
  %ext = zext i1 %r to i32
  ret i32 %ext
}
declare i1 @llvm.experimental.constrained.fcmp.bf16(bfloat, bfloat, metadata, metadata)
attributes #0 = { strictfp }
```

Vector version (also crashes):

```llvm
define <4 x i1> @t(<4 x bfloat> %a, <4 x bfloat> %b) #0 {
  %r = call <4 x i1> @llvm.experimental.constrained.fcmp.v4bf16(<4 x bfloat> %a, <4 x bfloat> %b, metadata !"olt", metadata !"fpexcept.strict")
  ret <4 x i1> %r
}
declare <4 x i1> @llvm.experimental.constrained.fcmp.v4bf16(<4 x bfloat>, <4 x bfloat>, metadata, metadata)
attributes #0 = { strictfp }
```

Signaling variant (`fcmps`) also crashes identically.

## Command

```
llc -O2 -mtriple=x86_64-linux-gnu t39_red.ll -o -
```

Also crashes with `-mattr=+avx2`, `-mattr=+avx512bf16`, and many other feature combos. No special flags needed.

## Crash

```
LLVM ERROR: Do not know how to soft promote this operator's operand!
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ ...
Stack dump:
1. Running pass 'Function Pass Manager' on module 't39_red.ll'.
2. Running pass 'X86 DAG->DAG Instruction Selection' on function '@t'

Frames:
  llvm::report_fatal_error(llvm::Twine const&, bool) + 437
  (anon)  -> DAGTypeLegalizer SoftPromoteHalfOperand path
  llvm::SelectionDAG::LegalizeTypes() + 1399
  llvm::SelectionDAGISel::CodeGenAndEmitDAG()
  ...
```

Frame 9 (`0x00000000044f650f`) is inside `DAGTypeLegalizer::SoftPromoteHalfOperand`'s dispatch switch which is missing a case for `STRICT_FSETCC` / `STRICT_FSETCCS`.

## Root cause (hypothesis)

`lib/CodeGen/SelectionDAG/LegalizeFloatTypes.cpp::SoftPromoteHalfOperand` lacks a `STRICT_FSETCC` / `STRICT_FSETCCS` case. The non-strict `FSETCC` is handled (which is why `fcmp olt bfloat` works), but the strict-fp variants fall through to the `llvm_unreachable` / `report_fatal_error` arm.

The matching `SoftPromoteHalfResult` switch (see below) is likely missing strict-fp result cases too — `constrained.fadd.bf16` on the same path hits the result-side equivalent ICE.
