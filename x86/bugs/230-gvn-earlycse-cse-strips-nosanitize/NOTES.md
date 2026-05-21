# 230 — GVN/EarlyCSE strip `!nosanitize` from the stationary CSE leader

Component: `llvm/lib/Transforms/Utils/Local.cpp` lines ~3040-3043 (`MD_nosanitize` arm of `combineMetadata`)

Same family as #229. The `MD_nosanitize` arm unconditionally writes `K->setMetadata(MD_nosanitize, JMD)`. When J lacks `!nosanitize`, K's tag is stripped. The runtime-probe load that the frontend explicitly marked as not-to-be-instrumented now becomes subject to ASan/TSan/MSan instrumentation when those passes run after GVN/EarlyCSE.

## Reproducer

`opt -passes=gvn -S repro.ll` → `%a = load i32, ptr %p, align 4` (no `!nosanitize`).

## Severity

Correctness-adjacent (sanitizer-build false positives / unwanted instrumentation). Default x86 -O2.

## Fix

Guard with `if (DoesKMove)` in the `MD_nosanitize` arm.
