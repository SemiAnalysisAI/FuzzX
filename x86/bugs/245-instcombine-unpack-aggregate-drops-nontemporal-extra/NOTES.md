# 245 — InstCombine `unpackStoreToAggregate` drops `!nontemporal` AND `!invariant.group` on per-field stores (full enumeration vs #212)

Component: `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp` (`unpackStoreToAggregate`, both STRUCT and ARRAY multi-element branches: lines ~1380, 1426)

Sibling/superset of #212. Per-field `store` from an aggregate split loses multiple kinds:
- `!nontemporal`
- `!access_group`
- `!mem_parallel_loop_access`
- `!invariant.group`
- `!DIAssignID`

Only `setAAMetadata(SI.getAAMetadata())` is called; no general metadata copy.

## Reproducer

`opt -passes=instcombine -S repro.ll`

Input: `store %S %v, ptr %p, align 8, !nontemporal !0, !invariant.group !1`.
Output: two `store i64` ops with NO metadata.

## Severity

Default x86 -O2. NT hint AND invariant.group both silently dropped on aggregate stores. The `!invariant.group` loss is correctness-relevant (per #142 family).

## Fix

Use `copyMetadataForLoad`-equivalent for stores, or explicitly enumerate store-safe kinds including `MD_nontemporal`/`MD_invariant_group`/`MD_access_group`/`MD_mem_parallel_loop_access`/`MD_DIAssignID`.
