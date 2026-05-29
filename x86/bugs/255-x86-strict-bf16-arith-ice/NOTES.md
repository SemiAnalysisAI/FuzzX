# 255 — strict-fp constrained arithmetic on bf16 ICE (x86)

Component: `SelectionDAG` DAGTypeLegalizer `SoftPromoteHalfResult`.

Strict-fp arithmetic returning scalar (or vector) `bfloat`
(`llvm.experimental.constrained.fadd.bf16`, `.fmul`, `.fsub`, `.fdiv`,
`.sqrt`, `.fma`, …) crashes during type legalization: the
SoftPromoteHalfResult legalizer has no case for the STRICT_* arithmetic
opcodes when the result type is bf16. The non-strict equivalents lower fine.

## Crash (HEAD, assertions on)
```
LLVM ERROR: Do not know how to soft promote this operator's result!
... X86 DAG->DAG Instruction Selection ...
```

## Repro
`llc -O2 -mtriple=x86_64-linux-gnu repro.ll` — crashes. Default mattr, no flags.
Distinct root cause from #256 (operand-side soft-promote of strict fcmp).
Verified at HEAD.
