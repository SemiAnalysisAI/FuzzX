# 222 — `Expand IR instructions` pass ICE on vector `llvm.fpto{u,s}i.sat.<vecty>` with element width > target max

Component: `llvm/lib/CodeGen/ExpandIRInsts.cpp` lines ~1117-1149 (`scalarize`) plus dispatcher at lines 1213-1218.

The dispatcher enqueues `IntrinsicInst` for `fptoui_sat`/`fptosi_sat` requiring expansion, but `scalarize` handles only `BinaryOperator` and `CastInst`. A vector form of these intrinsics (e.g. `<2 x i256> @llvm.fptoui.sat.v2i256.v2f32`) where the element width exceeds the x86 max (`<=128`) reaches `scalarize` and crashes.

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll -o -`:

```
Stack dump:
0. Program arguments: llc -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll
2. Running pass 'Expand IR instructions' on function '@f'
[crash]
```

## Severity

Hard crash in the default x86 codegen pipeline. Reachable from any source-level vector cast through `__builtin_convertvector` with saturating semantics and >128-bit elements.

## Fix

Add an `IntrinsicInst` case to `scalarize` that handles `fptoui_sat`/`fptosi_sat` by lane-extracting and emitting one scalar intrinsic call per lane.
