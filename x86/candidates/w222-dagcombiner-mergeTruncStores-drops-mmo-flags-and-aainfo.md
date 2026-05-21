# w222: DAGCombiner mergeTruncStores drops MMO flags (nontemporal) and AAInfo (TBAA)

## Summary

`DAGCombiner::mergeTruncStores` fuses a chain of N narrow stores of the form
`store i8 (trunc (lshr i32 v, k*8)), p+k` into a single wider store
`store iN*8 v, p`. It builds the resulting store with the four-argument
`DAG.getStore(Chain, dl, Val, Ptr, PtrInfo, Align)` overload, which defaults
`MachineMemOperand::Flags = MONone` and `AAMDNodes = {}`.

As a result:

1. `MachineMemOperand::MONonTemporal` is dropped. If every byte store carried
   `!nontemporal`, the merged store is plain — the hardware streaming /
   write-combining hint that the source explicitly requested is lost.
2. `!tbaa`, `!alias.scope`, `!noalias` are all dropped. Downstream MI passes
   that consult MMO AAInfo lose the ability to disambiguate the merged store
   from surrounding memory.

## Source

File: `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp`
- `mergeTruncStores` at line 9929-9931:

```cpp
SDValue NewStore =
    DAG.getStore(Chain, DL, SourceValue, FirstStore->getBasePtr(),
                 FirstStore->getPointerInfo(), FirstStore->getAlign());
```

Compare against `SelectionDAG::getStore` signature in
`include/llvm/CodeGen/SelectionDAG.h:1572-1585`:
```cpp
SDValue getStore(SDValue Chain, const SDLoc &dl, SDValue Val, SDValue Ptr,
                 MachinePointerInfo PtrInfo, Align Alignment,
                 MachineMemOperand::Flags MMOFlags = MONone,
                 const AAMDNodes &AAInfo = AAMDNodes());
```

Both `MMOFlags` and `AAInfo` default to empty.

## Reproducer

```ll
; /tmp/w220/merge-trunc.ll
target triple = "x86_64-unknown-linux-gnu"

define void @t(ptr %p, i32 %v) {
  %p0 = getelementptr i8, ptr %p, i64 0
  %p1 = getelementptr i8, ptr %p, i64 1
  %p2 = getelementptr i8, ptr %p, i64 2
  %p3 = getelementptr i8, ptr %p, i64 3
  %b0 = trunc i32 %v to i8
  %s1 = lshr i32 %v, 8
  %b1 = trunc i32 %s1 to i8
  %s2 = lshr i32 %v, 16
  %b2 = trunc i32 %s2 to i8
  %s3 = lshr i32 %v, 24
  %b3 = trunc i32 %s3 to i8
  store i8 %b0, ptr %p0, align 1, !tbaa !0, !nontemporal !10
  store i8 %b1, ptr %p1, align 1, !tbaa !0, !nontemporal !10
  store i8 %b2, ptr %p2, align 1, !tbaa !0, !nontemporal !10
  store i8 %b3, ptr %p3, align 1, !tbaa !0, !nontemporal !10
  ret void
}

!0  = !{!1, !1, i64 0}
!1  = !{!"char", !2}
!2  = !{!"omnipotent char", !3}
!3  = !{!"Simple C/C++ TBAA"}
!10 = !{i32 1}
```

## Repro command

```
llc -mtriple=x86_64-unknown-linux-gnu -O2 \
    -stop-after=finalize-isel /tmp/w220/merge-trunc.ll -o -
```

## MIR diff

Original IR:
```
store i8 %b0, ptr %p01, align 1, !tbaa !0, !nontemporal !4
store i8 %b1, ptr %p1,  align 1, !tbaa !0, !nontemporal !4
store i8 %b2, ptr %p2,  align 1, !tbaa !0, !nontemporal !4
store i8 %b3, ptr %p3,  align 1, !tbaa !0, !nontemporal !4
```

After `mergeTruncStores`:
```
MOV32mr %0, 1, $noreg, 0, $noreg, %1 :: (store (s32) into %ir.p01, align 1)
```

The MMO is a plain `(store (s32) into %ir.p01, align 1)`. The
`non-temporal` flag is missing (compare to `nontemporal` MMOs which print
as `non-temporal store (s32) ...`). The `!tbaa` is also gone.

## Severity

- **Correctness risk for nontemporal:** the source program asked for streaming
  semantics so cache pollution would be minimized. The merged store loses
  that hint and goes through normal write-back caches.
- **Reordering risk for TBAA:** the merged store no longer carries the
  `char`/struct-field TBAA, so post-isel passes may reorder it against
  other accesses that they would have known to keep ordered.

## Suggested fix

Compute the union (for `MMOFlags`, preserving `MONonTemporal` only if every
source store has it) and the intersection (for `AAInfo`, via `AAInfo.concat`)
of the source stores. Pass them to the 8-arg `getStore` overload that takes
`MachineMemOperand::Flags` and `AAMDNodes`. The existing
`mergeStoresOfConstantsOrVecElts` (DAGCombiner.cpp:22651-22663, 22800) already
does this correctly and can be used as a template.
