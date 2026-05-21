# 117 — `fdiv X, -1.0` reduces to bare `xorps` sign-flip (no divsd) — sNaN passthrough

Component: DAGCombiner visitFDIV → visitFMUL → visitFSUB chain.
The in-source FIXME at `DAGCombiner.cpp:19057` (visitFSUB
`fsub -0.0, X → fneg X`) documents the missing `nnan` guard. Multiple IR
seeds (fmul X,-1.0; fdiv X,-1.0; etc.) all bottom out in the same FSUB→FNEG
sink and lower to a single `xorps sign_mask, %xmm0`.

For sNaN input, the hardware `divsd` would quiet to qNaN and raise invalid-op;
the lowered code just flips the sign bit, leaving an sNaN.

See bug 112 (fp_round/fp_extend round-trip), 115, 116 (simplifyFPBinop)
for the related fold family.
