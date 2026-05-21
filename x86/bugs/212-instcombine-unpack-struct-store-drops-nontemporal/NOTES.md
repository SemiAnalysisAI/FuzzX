# 212 — InstCombine `unpackStoreToAggregate` drops `!nontemporal` on per-field stores

Component: `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp` (unpackStoreToAggregate).

Mirror of #211 for stores. When `store %struct %v, ptr %p, !nontemporal !0` is split into per-field `store i32`, none of the new stores carry `!nontemporal`.

## Reproducer

`opt -passes=instcombine -S repro.ll` produces 2 plain `store i32` — no `!nontemporal`.

## Severity

Default x86 -O2. NT hint silently disappears on aggregate stores.

## Fix

Copy `!nontemporal` (and related kinds) to each per-field store.
