# 257 — strict-fp `constrained.fcmp[s].v2f128` ICE (x86)

Component: `SelectionDAG` LegalizeVectorTypes (vector-result expansion).

`llvm.experimental.constrained.fcmp.v2f128` / `.fcmps.v2f128` crashes during
type legalization: the vector-result expander does not know how to expand
STRICT_FSETCC / STRICT_FSETCCS when the operand type is a vector of fp128
(it must scalarize via libcall while threading the chain). The non-strict
`fcmp olt <2 x fp128>` compiles fine (scalarized to two libcalls).

## Crash (HEAD, assertions on)
```
LLVM ERROR: Do not know how to expand the result of this operator!
```

## Repro
`llc -O2 -mtriple=x86_64-linux-gnu repro.ll` — crashes. Default mattr, no flags.
Verified at HEAD.
