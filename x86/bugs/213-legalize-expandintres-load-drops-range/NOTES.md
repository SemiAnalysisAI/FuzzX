# 213 — LegalizeIntegerTypes `ExpandIntRes_NormalLoad` drops `!range` on split loads

Component: `llvm/lib/CodeGen/SelectionDAG/LegalizeIntegerTypes.cpp` (ExpandIntRes_LOAD / split path).

When an i128 load with `!range !0` is split into two i64 loads, the new MMOs are bare. The original `!range` is silently lost on both halves.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel repro.ll -o -`

Per the MIR: the two `MOV64rm` MMOs are `(load (s64) from %ir.p, align 16)` and `(load (s64) from %ir.p + 8, basealign 16)` — no range info.

## Severity

Default x86 -O2. Backend AA / known-bits propagation loses scope info from source-level `!range`.

## Fix

Forward AAInfo (which includes a slot for range) and MMOFlags to both `getLoad` calls in the split path.
