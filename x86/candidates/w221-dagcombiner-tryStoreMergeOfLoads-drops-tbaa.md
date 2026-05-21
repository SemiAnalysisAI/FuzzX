# w221: DAGCombiner tryStoreMergeOfLoads drops AAInfo (TBAA / alias.scope / noalias) and ranges on merged load+store

## Summary

`DAGCombiner::tryStoreMergeOfLoads` merges a run of adjacent loads-feeding-stores
into a single wider load and a single wider store. The new `DAG.getLoad(...)`
and `DAG.getStore(...)` calls pass only the alignment, pointer info, and a
hand-built `LdMMOFlags` / `StMMOFlags` (which carries `MONonTemporal` and
`MODereferenceable`). They do NOT pass any `AAInfo()` from the source loads
or stores. They also do NOT preserve `getRanges()` of the source loads.

As a result, even when every individual access carries `!tbaa`,
`!alias.scope`, `!noalias`, the merged load/store on the resulting
`MachineMemOperand` is fully unannotated. Downstream MachineScheduler,
MachineSink, post-RA scheduling, etc. lose the ability to reorder these
accesses against other unrelated memory ops.

## Source

File: `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp`
- `tryStoreMergeOfLoads` lines 23590-23625 (the `getLoad` / `getStore`
  / `getExtLoad` / `getTruncStore` calls):

```cpp
NewLoad = DAG.getLoad(
    JointMemOpVT, LoadDL, FirstLoad->getChain(), FirstLoad->getBasePtr(),
    FirstLoad->getPointerInfo(), FirstLoadAlign, LdMMOFlags);
...
NewStore = DAG.getStore(
    NewStoreChain, StoreDL, StoreOp, FirstInChain->getBasePtr(),
    CanReusePtrInfo ? FirstInChain->getPointerInfo()
                    : MachinePointerInfo(FirstStoreAS),
    FirstStoreAlign, StMMOFlags);
```

`getLoad` / `getStore` overloads that accept `AAMDNodes` and `MDNode *Ranges`
exist (see e.g. `DAGCombiner.cpp:14914` for the same form on a different fold),
but they are not used here.

## Reproducer

```ll
; /tmp/w220/merge-loads-tbaa.ll
target triple = "x86_64-unknown-linux-gnu"

define void @merge(ptr noalias %s, ptr noalias %d) {
  %p0 = getelementptr inbounds i32, ptr %s, i64 0
  %p1 = getelementptr inbounds i32, ptr %s, i64 1
  %d0 = getelementptr inbounds i32, ptr %d, i64 0
  %d1 = getelementptr inbounds i32, ptr %d, i64 1
  %v0 = load i32, ptr %p0, align 8, !tbaa !0, !alias.scope !4
  %v1 = load i32, ptr %p1, align 4, !tbaa !0, !alias.scope !4
  store i32 %v0, ptr %d0, align 8, !tbaa !0, !noalias !5
  store i32 %v1, ptr %d1, align 4, !tbaa !0, !noalias !5
  ret void
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2}
!2 = !{!"omnipotent char", !3}
!3 = !{!"Simple C/C++ TBAA"}
!4 = !{!6}
!5 = !{!7}
!6 = distinct !{!6, !8, !"scope1"}
!7 = distinct !{!7, !8, !"scope2"}
!8 = distinct !{!8, !"domain"}
```

## Repro command

```
llc -mtriple=x86_64-unknown-linux-gnu -O2 \
    -stop-after=finalize-isel /tmp/w220/merge-loads-tbaa.ll -o -
```

## MIR diff

Before merging (IR):
```
%v0 = load i32, ptr %p01, align 8, !tbaa !0, !alias.scope !4
%v1 = load i32, ptr %p1,  align 4, !tbaa !0, !alias.scope !4
store i32 %v0, ptr %d02, align 8, !tbaa !0, !noalias !7
store i32 %v1, ptr %d1,  align 4, !tbaa !0, !noalias !7
```

After (MIR after `tryStoreMergeOfLoads`):
```
%2:gr64 = MOV64rm %0, 1, $noreg, 0, $noreg :: (load (s64) from %ir.p01)
MOV64mr %1, 1, $noreg, 0, $noreg, killed %2 :: (store (s64) into %ir.d02)
```

No `!tbaa`, no `!alias.scope`, no `!noalias` on either MMO.

## Severity

Metadata loss. The TBAA tag for `int` survives intact on the IR loads and stores
but is silently erased by SelectionDAG. Post-isel passes that consult MMO
AAInfo (scheduler, sink, machine-LICM hoisting) lose information that the
source language and frontend explicitly preserved through all of mid-level
opt and into DAG construction.

## Suggested fix

In `tryStoreMergeOfLoads`, compute the intersected `AAMDNodes` of all
`LoadNodes` and the intersected `AAMDNodes` of all `StoreNodes`, and pass
them to the wider `DAG.getLoad` / `DAG.getStore` overload that accepts
`AAMDNodes` (and optional `MDNode *Ranges` for the load).
