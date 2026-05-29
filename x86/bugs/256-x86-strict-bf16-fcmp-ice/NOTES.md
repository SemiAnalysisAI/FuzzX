# 256 — strict-fp `constrained.fcmp[s].bf16` ICE (x86)

Component: `SelectionDAG` DAGTypeLegalizer `SoftPromoteHalfOperand`.

`llvm.experimental.constrained.fcmp.bf16` / `.fcmps.bf16` (scalar or vector
bf16) crashes during type legalization: the SoftPromoteHalfOperand legalizer
has no case for the strict-fp compare. The non-strict `fcmp olt bfloat`
compiles fine.

## Crash (HEAD, assertions on)
```
LLVM ERROR: Do not know how to soft promote this operator's operand!
```

## Repro
`llc -O2 -mtriple=x86_64-linux-gnu repro.ll` — crashes. Default mattr, no flags.
Distinct root cause from #255 (result-side soft-promote of strict arith).
Verified at HEAD.
