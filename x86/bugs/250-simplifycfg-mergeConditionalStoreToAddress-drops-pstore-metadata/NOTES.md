# 250 — SimplifyCFG `mergeConditionalStoreToAddress` drops PStore-only metadata; can violate `!invariant.group` contract

Component: `llvm/lib/Transforms/Utils/SimplifyCFG.cpp` line ~4409

The two-step pattern `combineMetadataForCSE(QStore, PStore, true); SI->copyMetadata(*QStore);` uses asymmetric `combineMetadata` (`Local.cpp:2937`) which iterates ONLY K's (QStore's) metadata kinds. Any metadata kind on PStore but not QStore is silently dropped: `!nontemporal`, `!tbaa`, `!noalias`, `!alias.scope`, `!fpmath`, `!nofpclass`, `!range`, `!nonnull`, `!align`, `!noundef`, `!access_group`, etc.

Worse: `!invariant.group` is *special-cased* in `combineMetadata` to be picked up from PStore (the eliminated side). But the merged store's value is `select q, QV, PV` — applying `!invariant.group` to a store with a value that depends on `q` violates the invariant.group contract (all stores to the same pointer carrying `!invariant.group` must store the same value). Potential miscompile when a downstream `!invariant.group` load is folded against the merged store.

## Reproducer

```ll
p.t:  store i32 %pv, ptr %addr, !nontemporal !0, !tbaa !1
q.t:  store i32 %qv, ptr %addr
```

`opt -passes=simplifycfg -S` →
```
%spec.select = select i1 %q, i32 %qv, i32 %pv
store i32 %spec.select, ptr %addr, align 4   ; !nontemporal, !tbaa LOST
```

## Severity

Default x86 -O2. Metadata-loss for several kinds; potential `!invariant.group` miscompile via the special-cased PStore pickup.

## Fix

Use a symmetric metadata merger that iterates the union of both stores' metadata kinds, and either drop `!invariant.group` entirely when the value depends on the merge condition, or refuse to merge when either store has `!invariant.group`.
