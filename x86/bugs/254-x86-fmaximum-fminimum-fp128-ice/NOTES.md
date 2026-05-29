# 254 — `llvm.maximum.f128` / `llvm.minimum.f128` ICE (x86)

Component: `SelectionDAG` LegalizeDAG / X86 fp128 lowering.

`llvm.maximum.f128` and `llvm.minimum.f128` (and the `llvm.vector.reduce.fmaximum/fminimum.vNf128`
reductions) crash the X86 backend during DAG legalization. The generic
FMAXIMUM/FMINIMUM expansion emits a `setcc` on the i128 bitcast of the fp128
input (to detect signed-zero / NaN edge cases); the i128 setcc / fp128 path is
left in an illegal-type state that isel cannot handle.

`llvm.maxnum.f128`/`llvm.minnum.f128` compile fine (libcall path `RTLIB::FMAX_F128`);
the stricter `maximum`/`minimum` variants were never given an f128 lowering.

## Crash (HEAD 023e7decf625, assertions on)
```
Assertion failed: (... "Unexpected illegal type!"), function LegalizeOp,
file LegalizeDAG.cpp, line 1004.
```
(In release: "Cannot select" during isel.)

## Repro
`llc -O2 -mtriple=x86_64-linux-gnu repro.ll` — crashes. Default mattr, no flags.
Verified at HEAD.
