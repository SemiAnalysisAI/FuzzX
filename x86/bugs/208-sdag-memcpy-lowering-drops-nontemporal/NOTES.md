# 208 — SelectionDAG `getMemcpyLoadsAndStores` drops `!nontemporal` from `llvm.memcpy`

Component: `llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp` lines ~9331-9332 (helper), with `SelectionDAGBuilder::visitMemCpyInst` (line 6711) failing to query `MD_nontemporal` and pass it through.

When the SDAG inline-expansion lowers an `llvm.memcpy` annotated `!nontemporal !0`, the per-chunk loads/stores receive MMOFlags computed only from `isVol`. MONonTemporal is silently dropped, so the x86 selector emits cached `VMOVUPS*`/`MOVAPS*` instead of `VMOVNTPSmr`/`MOVNTDQmr`. The wiring works for direct IR stores `store …, !nontemporal` — only the memcpy-lowering helper drops the bit.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -mattr=+avx2 -stop-after=finalize-isel repro.ll -o -`

Per the MIR: each `VMOVUPSYrm/mr` MMO is plain `(load/store (s256) from %ir.s, align 16)` — no `non-temporal` flag. Expected: each MMO should carry `non-temporal load/store`.

## Severity

Default x86 -O2. NT hint on memcpy intrinsics is the only sane way for source-language `__builtin_memcpy_nontemporal` to reach the MOVNT path; this drops it entirely.

## Fix

Plumb `MD_nontemporal` (and MOInvariant where applicable) from the call instruction into `getMemcpyLoadsAndStores` via an extra `MachineMemOperand::Flags` parameter.
