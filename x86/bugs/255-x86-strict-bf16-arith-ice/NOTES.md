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

## Fix investigation (2026-05-29)

A fix must add the `STRICT_*` arithmetic opcodes to the `SoftPromoteHalfResult`
dispatch and to `SoftPromoteHalfRes_BinOp`/`_UnaryOp`/`_FMAD`/`_ExpOp`, lowering
each as: (non-strict, exact) half->f32 extend -> strict op on f32 (threading the
chain) -> strict f32->half round.

**Blocker found via execution (x86 under Rosetta):** the structural lowering is
correct, but using `STRICT_FP_TO_BF16` for the round produces a WRONG VALUE
(e.g. strict `fadd(1.5, 2.25)` returns 2.0 instead of 3.75). `__truncsfbf2`
returns the bf16 in XMM0, but the `STRICT_FP_TO_BF16` i16 result is materialized
in EAX and `pinsrw`'d into XMM0, overwriting the real result with stale EAX. The
non-strict `FP_TO_BF16` round gives the correct value (3.75). So a complete fix
also needs to repair `STRICT_FP_TO_BF16`'s libcall-result handling (a separate,
deeper bug); a non-strict round is value-correct but drops the round's strict
exception semantics. Not landed pending that deeper fix.

## WONTFIX

The strict-fp constrained-intrinsic API is changing / being phased out and is
not widely used, so this strict-fp legalization gap is not worth fixing. (The
crash is real, but deprioritized.) See also #255's fix-investigation note re: a
deeper STRICT_FP_TO_BF16 result-ABI bug that any fix would also have to address.
