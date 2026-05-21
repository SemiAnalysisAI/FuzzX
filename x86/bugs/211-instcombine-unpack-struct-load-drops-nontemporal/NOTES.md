# 211 — InstCombine `unpackLoadToAggregate` drops `!nontemporal` (and `!access_group`/`!mem_parallel_loop_access`) on per-field loads

Component: `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp` (unpackLoadToAggregate path).

When an aggregate `load %struct, ptr %p` is split into per-field scalar loads, the new loads never receive the original load's `!nontemporal` (or related loop-parallelism metadata). For a struct with `!nontemporal !0`, the resulting per-field `load i32` instructions have no metadata — the NT hint disappears.

## Reproducer

`opt -passes=instcombine -S repro.ll` produces:
```
  %v.unpack = load i32, ptr %p, align 4              ; NO !nontemporal
  %v.unpack2 = load i32, ptr %v.elt1, align 4         ; NO !nontemporal
```

## Severity

Default x86 -O2.

## Fix

Copy the source load's poison-generating IDs + `!nontemporal` + `!access_group` + `!mem_parallel_loop_access` + `!tbaa` to each per-field load.
