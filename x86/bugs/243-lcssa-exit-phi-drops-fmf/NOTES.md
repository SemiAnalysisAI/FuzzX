# 243 — LCSSA-inserted exit `phi` drops FMF (`nnan`/`ninf`/`nsz`/`reassoc`) from the single incoming value

Component: `llvm/lib/Transforms/Utils/LCSSA.cpp` lines ~163-170

When LCSSA inserts an exit-block phi for a loop-defined FP value, the new phi is created with no FMF. `phi` IS an `FPMathOperator` (per `Operator.h:349-380`), and `Operator.cpp:67-70` lists `nnan`/`ninf` as poison-generating on FP phis. Downstream FMF-gated passes (LoopVectorize reduction matching, InstCombine FP folds) pessimize because the LCSSA phi looks weaker than its source.

## Reproducer

`opt -passes=lcssa -S repro.ll`

Source: `%y = fmul reassoc nnan ninf nsz float %a, %b`.
LCSSA emits: `%y.lcssa = phi float [ %y, %h ]` — no FMF on the phi.

## Severity

Default x86 -O2 (LCSSA runs as part of standard loop pipeline). Inhibits downstream FMF-gated optimizations.

## Fix

After creating the LCSSA phi, copy FMF from the source value: `cast<PHINode>(PN)->copyFastMathFlags(SrcInst)`.
